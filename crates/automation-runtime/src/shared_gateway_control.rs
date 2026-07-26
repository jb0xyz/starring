use std::fmt;
use std::num::{NonZeroU64, NonZeroUsize};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, Weak};

use tokio::sync::{mpsc, oneshot, watch};

const MAX_COMMAND_CAPACITY: usize = 64;
const MAX_LIFECYCLE_CAPACITY: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GatewayConnectionEpochV3(NonZeroU64);

impl GatewayConnectionEpochV3 {
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GatewayAdmissionRevisionV3(NonZeroU64);

impl GatewayAdmissionRevisionV3 {
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GatewayAdmissionSequenceV3(u64);

impl GatewayAdmissionSequenceV3 {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayReadyKindV3 {
    Ready,
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayDisconnectKindV3 {
    Close,
    Reconnect,
    SessionInvalidated,
    ReceiveError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayDrainCauseV3 {
    Commanded,
    ControlOrphaned,
    ConnectionEpochOverflow,
    AdmissionRevisionOverflow,
    AdmissionSequenceOverflow,
    LifecycleOverflow,
    LifecycleClosed,
    RuntimeFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayInvalidationSignalV3 {
    AdmissionPaused,
    Disconnected(GatewayDisconnectKindV3),
    Draining(GatewayDrainCauseV3),
    Stopped(GatewayDrainCauseV3),
    ControlOrphaned,
}

pub trait GatewaySynchronousInvalidatorV3: Send + Sync {
    fn invalidate(&self, signal: GatewayInvalidationSignalV3);
}

#[derive(Clone, Default)]
struct GatewaySynchronousInvalidationHookV3 {
    state: Option<Arc<GatewaySynchronousInvalidationStateV3>>,
}

struct GatewaySynchronousInvalidationStateV3 {
    invalidator: Arc<dyn GatewaySynchronousInvalidatorV3>,
    healthy: AtomicBool,
    serial: StdMutex<()>,
}

impl GatewaySynchronousInvalidationHookV3 {
    fn new(invalidator: impl GatewaySynchronousInvalidatorV3 + 'static) -> Self {
        Self {
            state: Some(Arc::new(GatewaySynchronousInvalidationStateV3 {
                invalidator: Arc::new(invalidator),
                healthy: AtomicBool::new(true),
                serial: StdMutex::new(()),
            })),
        }
    }

    fn invalidate(&self, signal: GatewayInvalidationSignalV3) {
        let Some(state) = &self.state else {
            return;
        };
        let _serial = state
            .serial
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.healthy.load(Ordering::Acquire) {
            return;
        }
        if catch_unwind(AssertUnwindSafe(|| state.invalidator.invalidate(signal))).is_err() {
            state.healthy.store(false, Ordering::Release);
        }
    }

    fn is_healthy(&self) -> bool {
        self.state
            .as_ref()
            .is_none_or(|state| state.healthy.load(Ordering::Acquire))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayPausedConnectionV3 {
    Starting,
    Connected {
        epoch: GatewayConnectionEpochV3,
        kind: GatewayReadyKindV3,
    },
    Disconnected {
        last_epoch: Option<GatewayConnectionEpochV3>,
        kind: GatewayDisconnectKindV3,
    },
}

impl GatewayPausedConnectionV3 {
    fn current_epoch(self) -> Option<GatewayConnectionEpochV3> {
        match self {
            Self::Starting => None,
            Self::Connected { epoch, .. } => Some(epoch),
            Self::Disconnected { last_epoch, .. } => last_epoch,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayConnectionStateV3 {
    Starting,
    Connected {
        epoch: GatewayConnectionEpochV3,
        kind: GatewayReadyKindV3,
    },
    Disconnected {
        last_epoch: Option<GatewayConnectionEpochV3>,
        kind: GatewayDisconnectKindV3,
    },
    Paused {
        connection: GatewayPausedConnectionV3,
    },
    Draining {
        last_epoch: Option<GatewayConnectionEpochV3>,
        cause: GatewayDrainCauseV3,
    },
    Stopped {
        last_epoch: Option<GatewayConnectionEpochV3>,
        cause: GatewayDrainCauseV3,
    },
}

impl GatewayConnectionStateV3 {
    pub fn current_epoch(self) -> Option<GatewayConnectionEpochV3> {
        match self {
            Self::Starting => None,
            Self::Connected { epoch, .. } => Some(epoch),
            Self::Disconnected { last_epoch, .. }
            | Self::Draining { last_epoch, .. }
            | Self::Stopped { last_epoch, .. } => last_epoch,
            Self::Paused { connection } => connection.current_epoch(),
        }
    }

    pub fn admits_interactions(self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayLifecycleEventV3 {
    Starting,
    Connected {
        epoch: GatewayConnectionEpochV3,
        kind: GatewayReadyKindV3,
        paused: bool,
    },
    Disconnected {
        last_epoch: Option<GatewayConnectionEpochV3>,
        kind: GatewayDisconnectKindV3,
        paused: bool,
    },
    Paused {
        epoch: Option<GatewayConnectionEpochV3>,
    },
    AdmissionResumed {
        epoch: GatewayConnectionEpochV3,
    },
    Draining {
        last_epoch: Option<GatewayConnectionEpochV3>,
        cause: GatewayDrainCauseV3,
    },
    Stopped {
        last_epoch: Option<GatewayConnectionEpochV3>,
        cause: GatewayDrainCauseV3,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayControlConfigV3 {
    command_capacity: NonZeroUsize,
    lifecycle_capacity: NonZeroUsize,
}

impl GatewayControlConfigV3 {
    pub fn new(
        command_capacity: NonZeroUsize,
        lifecycle_capacity: NonZeroUsize,
    ) -> Result<Self, GatewayControlConfigurationErrorV3> {
        if command_capacity.get() > MAX_COMMAND_CAPACITY {
            return Err(GatewayControlConfigurationErrorV3::CommandCapacity);
        }
        if lifecycle_capacity.get() > MAX_LIFECYCLE_CAPACITY {
            return Err(GatewayControlConfigurationErrorV3::LifecycleCapacity);
        }
        Ok(Self {
            command_capacity,
            lifecycle_capacity,
        })
    }
}

impl Default for GatewayControlConfigV3 {
    fn default() -> Self {
        Self {
            command_capacity: NonZeroUsize::new(8).expect("command capacity is non-zero"),
            lifecycle_capacity: NonZeroUsize::new(64).expect("lifecycle capacity is non-zero"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayAdmissionPolicyV3 {
    ResumeOnConnect,
    ExplicitResumeAfterEveryConnect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GatewayControlConfigurationErrorV3 {
    #[error("gateway command capacity is invalid")]
    CommandCapacity,
    #[error("gateway lifecycle capacity is invalid")]
    LifecycleCapacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GatewayControlTransitionErrorV3 {
    #[error("gateway connection epoch is stale")]
    StaleConnectionEpoch,
    #[error("gateway admission snapshot is stale")]
    StaleAdmissionSnapshot,
    #[error("gateway control owner is unavailable")]
    ControlOrphaned,
    #[error("gateway is not connected")]
    NotConnected,
    #[error("gateway admission is paused")]
    AdmissionPaused,
    #[error("gateway admission is not paused")]
    AdmissionNotPaused,
    #[error("gateway is draining")]
    Draining,
    #[error("gateway is stopped")]
    Stopped,
    #[error("gateway connection epoch overflowed")]
    ConnectionEpochOverflow,
    #[error("gateway admission revision overflowed")]
    AdmissionRevisionOverflow,
    #[error("gateway admission sequence overflowed")]
    AdmissionSequenceOverflow,
    #[error("gateway lifecycle queue overflowed")]
    LifecycleOverflow,
    #[error("gateway lifecycle observer is unavailable")]
    LifecycleClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GatewayControlErrorV3 {
    #[error("gateway runtime is no longer accepting commands")]
    RuntimeStopped,
    #[error("gateway command acknowledgement was lost")]
    AcknowledgementLost,
    #[error(transparent)]
    Transition(#[from] GatewayControlTransitionErrorV3),
}

#[derive(Clone)]
pub struct GatewayPauseTokenV3 {
    control_alive: Weak<AtomicBool>,
    epoch: Option<GatewayConnectionEpochV3>,
    admission_revision: GatewayAdmissionRevisionV3,
    transition_sequence: GatewayAdmissionSequenceV3,
}

impl fmt::Debug for GatewayPauseTokenV3 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayPauseTokenV3")
            .field("epoch", &self.epoch)
            .field("admission_revision", &self.admission_revision)
            .field("transition_sequence", &self.transition_sequence)
            .finish()
    }
}

impl PartialEq for GatewayPauseTokenV3 {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.control_alive, &other.control_alive)
            && self.epoch == other.epoch
            && self.admission_revision == other.admission_revision
            && self.transition_sequence == other.transition_sequence
    }
}

impl Eq for GatewayPauseTokenV3 {}

impl GatewayPauseTokenV3 {
    pub fn epoch(&self) -> Option<GatewayConnectionEpochV3> {
        self.epoch
    }

    pub fn admission_revision(&self) -> GatewayAdmissionRevisionV3 {
        self.admission_revision
    }

    pub fn transition_sequence(&self) -> GatewayAdmissionSequenceV3 {
        self.transition_sequence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayCommandAckV3 {
    Paused {
        epoch: Option<GatewayConnectionEpochV3>,
        resume_token: GatewayPauseTokenV3,
    },
    AdmissionResumed {
        epoch: GatewayConnectionEpochV3,
    },
    Draining {
        last_epoch: Option<GatewayConnectionEpochV3>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatewayRuntimeCommandOutcomeV3 {
    Applied(GatewayCommandAckV3),
    Rejected(GatewayControlTransitionErrorV3),
    ControlOrphaned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayReadyLeaseV3 {
    epoch: GatewayConnectionEpochV3,
    kind: GatewayReadyKindV3,
    admission_revision: GatewayAdmissionRevisionV3,
    connected_event_sequence: GatewayAdmissionSequenceV3,
    resume_sequence: GatewayAdmissionSequenceV3,
}

impl GatewayReadyLeaseV3 {
    pub fn epoch(self) -> GatewayConnectionEpochV3 {
        self.epoch
    }

    pub fn kind(self) -> GatewayReadyKindV3 {
        self.kind
    }

    pub fn admission_revision(self) -> GatewayAdmissionRevisionV3 {
        self.admission_revision
    }

    pub fn connected_event_sequence(self) -> GatewayAdmissionSequenceV3 {
        self.connected_event_sequence
    }

    pub fn resume_sequence(self) -> GatewayAdmissionSequenceV3 {
        self.resume_sequence
    }

    pub fn was_explicitly_resumed(self) -> bool {
        self.resume_sequence > self.connected_event_sequence
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayAdmissionSnapshotV3 {
    connection: GatewayConnectionStateV3,
    admission_revision: GatewayAdmissionRevisionV3,
    transition_sequence: GatewayAdmissionSequenceV3,
    connected_event_sequence: Option<GatewayAdmissionSequenceV3>,
    resume_sequence: Option<GatewayAdmissionSequenceV3>,
}

impl GatewayAdmissionSnapshotV3 {
    pub fn connection(self) -> GatewayConnectionStateV3 {
        self.connection
    }

    pub fn admission_revision(self) -> GatewayAdmissionRevisionV3 {
        self.admission_revision
    }

    pub fn transition_sequence(self) -> GatewayAdmissionSequenceV3 {
        self.transition_sequence
    }

    pub fn connected_event_sequence(self) -> Option<GatewayAdmissionSequenceV3> {
        self.connected_event_sequence
    }

    pub fn resume_sequence(self) -> Option<GatewayAdmissionSequenceV3> {
        self.resume_sequence
    }
}

enum GatewayCommandV3 {
    Pause {
        acknowledgement:
            oneshot::Sender<Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>>,
    },
    Resume {
        pause_token: GatewayPauseTokenV3,
        acknowledgement:
            oneshot::Sender<Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>>,
    },
    Drain {
        acknowledgement:
            oneshot::Sender<Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>>,
    },
}

pub struct SharedGatewayControlV3 {
    commands: mpsc::Sender<GatewayCommandV3>,
    lifecycle: mpsc::Receiver<GatewayLifecycleEventV3>,
    connection: watch::Receiver<GatewayConnectionStateV3>,
    admission: watch::Receiver<GatewayAdmissionSnapshotV3>,
    control_alive: Arc<AtomicBool>,
    invalidation: GatewaySynchronousInvalidationHookV3,
}

#[derive(Clone)]
pub struct GatewayConnectionObserverV3 {
    admission: watch::Receiver<GatewayAdmissionSnapshotV3>,
    control_alive: Arc<AtomicBool>,
    invalidation: GatewaySynchronousInvalidationHookV3,
}

impl GatewayConnectionObserverV3 {
    pub fn current_connection(&self) -> GatewayConnectionStateV3 {
        self.current_admission_snapshot().connection()
    }

    pub fn current_admission_snapshot(&self) -> GatewayAdmissionSnapshotV3 {
        *self.admission.borrow()
    }

    pub async fn connection_changed(&mut self) -> Option<GatewayConnectionStateV3> {
        self.admission_snapshot_changed()
            .await
            .map(GatewayAdmissionSnapshotV3::connection)
    }

    pub async fn admission_snapshot_changed(&mut self) -> Option<GatewayAdmissionSnapshotV3> {
        self.admission
            .changed()
            .await
            .ok()
            .map(|()| *self.admission.borrow_and_update())
    }

    pub fn issue_ready_lease(
        &self,
        expected_epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayReadyLeaseV3, GatewayControlTransitionErrorV3> {
        if !self.control_alive.load(Ordering::Acquire) || !self.invalidation.is_healthy() {
            return Err(GatewayControlTransitionErrorV3::ControlOrphaned);
        }
        issue_ready_lease(self.current_admission_snapshot(), expected_epoch)
    }

    pub fn ready_lease_is_current(&self, lease: &GatewayReadyLeaseV3) -> bool {
        self.control_alive.load(Ordering::Acquire)
            && self.invalidation.is_healthy()
            && ready_lease_is_current(self.current_admission_snapshot(), lease)
    }
}

impl SharedGatewayControlV3 {
    pub async fn pause_admission(&self) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(GatewayCommandV3::Pause {
                acknowledgement: sender,
            })
            .await
            .map_err(|_| GatewayControlErrorV3::RuntimeStopped)?;
        receiver
            .await
            .map_err(|_| GatewayControlErrorV3::AcknowledgementLost)?
            .map_err(GatewayControlErrorV3::Transition)
    }

    pub async fn resume_admission(
        &self,
        pause_token: &GatewayPauseTokenV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        if !self.invalidation.is_healthy() {
            return Err(GatewayControlTransitionErrorV3::ControlOrphaned.into());
        }
        if !Weak::ptr_eq(
            &pause_token.control_alive,
            &Arc::downgrade(&self.control_alive),
        ) {
            return Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot.into());
        }
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(GatewayCommandV3::Resume {
                pause_token: pause_token.clone(),
                acknowledgement: sender,
            })
            .await
            .map_err(|_| GatewayControlErrorV3::RuntimeStopped)?;
        receiver
            .await
            .map_err(|_| GatewayControlErrorV3::AcknowledgementLost)?
            .map_err(GatewayControlErrorV3::Transition)
    }

    pub async fn begin_drain(&self) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(GatewayCommandV3::Drain {
                acknowledgement: sender,
            })
            .await
            .map_err(|_| GatewayControlErrorV3::RuntimeStopped)?;
        receiver
            .await
            .map_err(|_| GatewayControlErrorV3::AcknowledgementLost)?
            .map_err(GatewayControlErrorV3::Transition)
    }

    pub fn current_connection(&self) -> GatewayConnectionStateV3 {
        self.current_admission_snapshot().connection()
    }

    pub fn current_admission_snapshot(&self) -> GatewayAdmissionSnapshotV3 {
        *self.admission.borrow()
    }

    pub fn connection_watch(&self) -> watch::Receiver<GatewayConnectionStateV3> {
        self.connection.clone()
    }

    pub fn admission_snapshot_watch(&self) -> watch::Receiver<GatewayAdmissionSnapshotV3> {
        self.admission.clone()
    }

    pub fn connection_observer(&self) -> GatewayConnectionObserverV3 {
        GatewayConnectionObserverV3 {
            admission: self.admission.clone(),
            control_alive: self.control_alive.clone(),
            invalidation: self.invalidation.clone(),
        }
    }

    pub async fn connection_changed(&mut self) -> Option<GatewayConnectionStateV3> {
        self.connection
            .changed()
            .await
            .ok()
            .map(|()| *self.connection.borrow_and_update())
    }

    pub async fn next_lifecycle(&mut self) -> Option<GatewayLifecycleEventV3> {
        self.lifecycle.recv().await
    }

    pub fn issue_ready_lease(
        &self,
        expected_epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayReadyLeaseV3, GatewayControlTransitionErrorV3> {
        if !self.control_alive.load(Ordering::Acquire) || !self.invalidation.is_healthy() {
            return Err(GatewayControlTransitionErrorV3::ControlOrphaned);
        }
        issue_ready_lease(self.current_admission_snapshot(), expected_epoch)
    }

    pub fn ready_lease_is_current(&self, lease: &GatewayReadyLeaseV3) -> bool {
        self.control_alive.load(Ordering::Acquire)
            && self.invalidation.is_healthy()
            && ready_lease_is_current(self.current_admission_snapshot(), lease)
    }
}

impl Drop for SharedGatewayControlV3 {
    fn drop(&mut self) {
        self.invalidation
            .invalidate(GatewayInvalidationSignalV3::ControlOrphaned);
        self.control_alive.store(false, Ordering::Release);
    }
}

fn issue_ready_lease(
    snapshot: GatewayAdmissionSnapshotV3,
    expected_epoch: GatewayConnectionEpochV3,
) -> Result<GatewayReadyLeaseV3, GatewayControlTransitionErrorV3> {
    match snapshot.connection {
        GatewayConnectionStateV3::Connected { epoch, .. } if epoch != expected_epoch => {
            Err(GatewayControlTransitionErrorV3::StaleConnectionEpoch)
        }
        GatewayConnectionStateV3::Connected { epoch, kind } => {
            let connected_event_sequence = snapshot
                .connected_event_sequence
                .ok_or(GatewayControlTransitionErrorV3::AdmissionPaused)?;
            let resume_sequence = snapshot
                .resume_sequence
                .filter(|resume| *resume >= connected_event_sequence)
                .ok_or(GatewayControlTransitionErrorV3::AdmissionPaused)?;
            Ok(GatewayReadyLeaseV3 {
                epoch,
                kind,
                admission_revision: snapshot.admission_revision,
                connected_event_sequence,
                resume_sequence,
            })
        }
        GatewayConnectionStateV3::Paused { .. } => {
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        }
        GatewayConnectionStateV3::Starting | GatewayConnectionStateV3::Disconnected { .. } => {
            Err(GatewayControlTransitionErrorV3::NotConnected)
        }
        GatewayConnectionStateV3::Draining { .. } => Err(GatewayControlTransitionErrorV3::Draining),
        GatewayConnectionStateV3::Stopped { .. } => Err(GatewayControlTransitionErrorV3::Stopped),
    }
}

fn ready_lease_is_current(
    snapshot: GatewayAdmissionSnapshotV3,
    lease: &GatewayReadyLeaseV3,
) -> bool {
    matches!(
        snapshot.connection,
        GatewayConnectionStateV3::Connected { epoch, kind }
            if epoch == lease.epoch
                && kind == lease.kind
                && snapshot.admission_revision == lease.admission_revision
                && snapshot.connected_event_sequence == Some(lease.connected_event_sequence)
                && snapshot.resume_sequence == Some(lease.resume_sequence)
    )
}

#[derive(Clone, Copy)]
enum GatewayAdmissionEvidenceUpdateV3 {
    Preserve,
    Connected { admitted: bool },
    Resumed,
}

pub struct SharedGatewayRuntimeControlV3 {
    commands: mpsc::Receiver<GatewayCommandV3>,
    lifecycle: mpsc::Sender<GatewayLifecycleEventV3>,
    connection: watch::Sender<GatewayConnectionStateV3>,
    admission: watch::Sender<GatewayAdmissionSnapshotV3>,
    state: GatewayConnectionStateV3,
    admission_snapshot: GatewayAdmissionSnapshotV3,
    control_alive: Arc<AtomicBool>,
    invalidation: GatewaySynchronousInvalidationHookV3,
    last_issued_epoch: u64,
    admission_policy: GatewayAdmissionPolicyV3,
}

pub fn shared_gateway_control_channel_v3(
    config: GatewayControlConfigV3,
) -> (SharedGatewayControlV3, SharedGatewayRuntimeControlV3) {
    shared_gateway_control_channel_with_policy_v3(config, GatewayAdmissionPolicyV3::ResumeOnConnect)
}

pub fn shared_gateway_control_channel_with_policy_v3(
    config: GatewayControlConfigV3,
    admission_policy: GatewayAdmissionPolicyV3,
) -> (SharedGatewayControlV3, SharedGatewayRuntimeControlV3) {
    shared_gateway_control_channel_inner_v3(
        config,
        admission_policy,
        GatewaySynchronousInvalidationHookV3::default(),
    )
}

pub fn shared_gateway_control_channel_with_policy_and_invalidator_v3<I>(
    config: GatewayControlConfigV3,
    admission_policy: GatewayAdmissionPolicyV3,
    invalidator: I,
) -> (SharedGatewayControlV3, SharedGatewayRuntimeControlV3)
where
    I: GatewaySynchronousInvalidatorV3 + 'static,
{
    shared_gateway_control_channel_inner_v3(
        config,
        admission_policy,
        GatewaySynchronousInvalidationHookV3::new(invalidator),
    )
}

fn shared_gateway_control_channel_inner_v3(
    config: GatewayControlConfigV3,
    admission_policy: GatewayAdmissionPolicyV3,
    invalidation: GatewaySynchronousInvalidationHookV3,
) -> (SharedGatewayControlV3, SharedGatewayRuntimeControlV3) {
    let (command_sender, command_receiver) = mpsc::channel(config.command_capacity.get());
    let (lifecycle_sender, lifecycle_receiver) = mpsc::channel(config.lifecycle_capacity.get());
    let initial = match admission_policy {
        GatewayAdmissionPolicyV3::ResumeOnConnect => GatewayConnectionStateV3::Starting,
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect => {
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Starting,
            }
        }
    };
    let (connection_sender, connection_receiver) = watch::channel(initial);
    let initial_admission_revision = GatewayAdmissionRevisionV3(NonZeroU64::MIN);
    let initial_admission_snapshot = GatewayAdmissionSnapshotV3 {
        connection: initial,
        admission_revision: initial_admission_revision,
        transition_sequence: GatewayAdmissionSequenceV3::default(),
        connected_event_sequence: None,
        resume_sequence: None,
    };
    let (admission_sender, admission_receiver) = watch::channel(initial_admission_snapshot);
    let control_alive = Arc::new(AtomicBool::new(true));
    lifecycle_sender
        .try_send(GatewayLifecycleEventV3::Starting)
        .expect("validated lifecycle capacity accepts the initial state");
    (
        SharedGatewayControlV3 {
            commands: command_sender,
            lifecycle: lifecycle_receiver,
            connection: connection_receiver,
            admission: admission_receiver,
            control_alive: control_alive.clone(),
            invalidation: invalidation.clone(),
        },
        SharedGatewayRuntimeControlV3 {
            commands: command_receiver,
            lifecycle: lifecycle_sender,
            connection: connection_sender,
            admission: admission_sender,
            state: initial,
            admission_snapshot: initial_admission_snapshot,
            control_alive,
            invalidation,
            last_issued_epoch: 0,
            admission_policy,
        },
    )
}

impl SharedGatewayRuntimeControlV3 {
    pub fn current_connection(&self) -> GatewayConnectionStateV3 {
        self.state
    }

    pub(crate) fn connection_observer(&self) -> GatewayConnectionObserverV3 {
        GatewayConnectionObserverV3 {
            admission: self.admission.subscribe(),
            control_alive: self.control_alive.clone(),
            invalidation: self.invalidation.clone(),
        }
    }

    pub(crate) fn begin_runtime_failure_drain(&mut self) {
        if !matches!(
            self.state,
            GatewayConnectionStateV3::Draining { .. } | GatewayConnectionStateV3::Stopped { .. }
        ) {
            self.fail_closed(GatewayDrainCauseV3::RuntimeFailure);
        }
    }

    pub fn mark_connected(
        &mut self,
        kind: GatewayReadyKindV3,
    ) -> Result<GatewayConnectionEpochV3, GatewayControlTransitionErrorV3> {
        self.require_running()?;
        let next = self
            .last_issued_epoch
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(GatewayConnectionEpochV3);
        let Some(epoch) = next else {
            self.fail_closed(GatewayDrainCauseV3::ConnectionEpochOverflow);
            return Err(GatewayControlTransitionErrorV3::ConnectionEpochOverflow);
        };
        let paused = matches!(
            self.admission_policy,
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect
        ) || matches!(self.state, GatewayConnectionStateV3::Paused { .. });
        let state = if paused {
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected { epoch, kind },
            }
        } else {
            GatewayConnectionStateV3::Connected { epoch, kind }
        };
        self.publish_transition(
            state,
            GatewayLifecycleEventV3::Connected {
                epoch,
                kind,
                paused,
            },
            self.admission_snapshot.admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Connected { admitted: !paused },
        )?;
        self.last_issued_epoch = epoch.get();
        Ok(epoch)
    }

    pub fn mark_disconnected(
        &mut self,
        kind: GatewayDisconnectKindV3,
    ) -> Result<(), GatewayControlTransitionErrorV3> {
        self.require_running()?;
        let last_epoch = self.state.current_epoch();
        let explicit_resume = matches!(
            self.admission_policy,
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect
        );
        let paused =
            explicit_resume || matches!(self.state, GatewayConnectionStateV3::Paused { .. });
        let state = if paused {
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Disconnected { last_epoch, kind },
            }
        } else {
            GatewayConnectionStateV3::Disconnected { last_epoch, kind }
        };
        let admission_revision = if explicit_resume {
            self.next_admission_revision()?
        } else {
            self.admission_snapshot.admission_revision
        };
        self.publish_transition(
            state,
            GatewayLifecycleEventV3::Disconnected {
                last_epoch,
                kind,
                paused,
            },
            admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        )?;
        Ok(())
    }

    pub async fn process_next_command(&mut self) -> GatewayRuntimeCommandOutcomeV3 {
        let Some(command) = self.commands.recv().await else {
            self.fail_closed(GatewayDrainCauseV3::ControlOrphaned);
            return GatewayRuntimeCommandOutcomeV3::ControlOrphaned;
        };
        if !self.control_alive.load(Ordering::Acquire)
            || self.commands.is_closed()
            || !self.invalidation.is_healthy()
        {
            self.fail_closed(GatewayDrainCauseV3::ControlOrphaned);
            return GatewayRuntimeCommandOutcomeV3::ControlOrphaned;
        }
        match command {
            GatewayCommandV3::Pause { acknowledgement } => {
                let result = self.pause();
                let outcome = command_outcome(&result);
                let _ = acknowledgement.send(result);
                outcome
            }
            GatewayCommandV3::Resume {
                pause_token,
                acknowledgement,
            } => {
                let result = if acknowledgement.is_closed() {
                    Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot)
                } else {
                    self.resume(&pause_token)
                };
                if !self.control_alive.load(Ordering::Acquire) {
                    self.fail_closed(GatewayDrainCauseV3::ControlOrphaned);
                    let _ =
                        acknowledgement.send(Err(GatewayControlTransitionErrorV3::ControlOrphaned));
                    return GatewayRuntimeCommandOutcomeV3::ControlOrphaned;
                }
                let outcome = command_outcome(&result);
                let _ = acknowledgement.send(result);
                outcome
            }
            GatewayCommandV3::Drain { acknowledgement } => {
                let result = self.drain();
                let outcome = command_outcome(&result);
                let _ = acknowledgement.send(result);
                outcome
            }
        }
    }

    pub fn mark_stopped(&mut self) -> Result<(), GatewayControlTransitionErrorV3> {
        if matches!(self.state, GatewayConnectionStateV3::Stopped { .. }) {
            return Ok(());
        }
        let last_epoch = self.state.current_epoch();
        let cause = match self.state {
            GatewayConnectionStateV3::Draining { cause, .. } => cause,
            _ => GatewayDrainCauseV3::RuntimeFailure,
        };
        let state = GatewayConnectionStateV3::Stopped { last_epoch, cause };
        let event = GatewayLifecycleEventV3::Stopped { last_epoch, cause };
        let snapshot = match self.next_admission_snapshot(
            state,
            self.admission_snapshot.admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let cause = GatewayDrainCauseV3::AdmissionSequenceOverflow;
                self.replace_without_sequence(GatewayConnectionStateV3::Stopped {
                    last_epoch,
                    cause,
                });
                return Err(error);
            }
        };
        let lifecycle = self.lifecycle.clone();
        let result = match lifecycle.try_reserve() {
            Ok(permit) => {
                self.replace_snapshot(snapshot);
                permit.send(event);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                self.replace_snapshot(snapshot);
                Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                self.replace_snapshot(snapshot);
                Err(GatewayControlTransitionErrorV3::LifecycleClosed)
            }
        };
        result
    }

    fn pause(&mut self) -> Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3> {
        let connection = match self.state {
            GatewayConnectionStateV3::Starting => GatewayPausedConnectionV3::Starting,
            GatewayConnectionStateV3::Connected { epoch, kind } => {
                GatewayPausedConnectionV3::Connected { epoch, kind }
            }
            GatewayConnectionStateV3::Disconnected { last_epoch, kind } => {
                GatewayPausedConnectionV3::Disconnected { last_epoch, kind }
            }
            GatewayConnectionStateV3::Paused { connection } => connection,
            GatewayConnectionStateV3::Draining { .. } => {
                return Err(GatewayControlTransitionErrorV3::Draining)
            }
            GatewayConnectionStateV3::Stopped { .. } => {
                return Err(GatewayControlTransitionErrorV3::Stopped)
            }
        };
        let epoch = connection.current_epoch();
        let admission_revision = self.next_admission_revision()?;
        self.publish_transition(
            GatewayConnectionStateV3::Paused { connection },
            GatewayLifecycleEventV3::Paused { epoch },
            admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        )?;
        Ok(GatewayCommandAckV3::Paused {
            epoch,
            resume_token: GatewayPauseTokenV3 {
                control_alive: Arc::downgrade(&self.control_alive),
                epoch,
                admission_revision: self.admission_snapshot.admission_revision,
                transition_sequence: self.admission_snapshot.transition_sequence,
            },
        })
    }

    fn resume(
        &mut self,
        pause_token: &GatewayPauseTokenV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3> {
        if !Weak::ptr_eq(
            &pause_token.control_alive,
            &Arc::downgrade(&self.control_alive),
        ) {
            return Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot);
        }
        let expected_epoch = pause_token
            .epoch
            .ok_or(GatewayControlTransitionErrorV3::NotConnected)?;
        match self.state {
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected { epoch, kind },
            } if epoch == expected_epoch => {
                self.require_admission_snapshot(pause_token)?;
                self.publish_transition(
                    GatewayConnectionStateV3::Connected { epoch, kind },
                    GatewayLifecycleEventV3::AdmissionResumed { epoch },
                    self.admission_snapshot.admission_revision,
                    GatewayAdmissionEvidenceUpdateV3::Resumed,
                )?;
                Ok(GatewayCommandAckV3::AdmissionResumed { epoch })
            }
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected { .. },
            }
            | GatewayConnectionStateV3::Connected { .. } => {
                if self.state.current_epoch() == Some(expected_epoch)
                    && matches!(self.state, GatewayConnectionStateV3::Connected { .. })
                {
                    self.require_admission_snapshot(pause_token)?;
                    Ok(GatewayCommandAckV3::AdmissionResumed {
                        epoch: expected_epoch,
                    })
                } else {
                    Err(GatewayControlTransitionErrorV3::StaleConnectionEpoch)
                }
            }
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Starting,
            }
            | GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Disconnected { .. },
            }
            | GatewayConnectionStateV3::Starting
            | GatewayConnectionStateV3::Disconnected { .. } => {
                Err(GatewayControlTransitionErrorV3::NotConnected)
            }
            GatewayConnectionStateV3::Draining { .. } => {
                Err(GatewayControlTransitionErrorV3::Draining)
            }
            GatewayConnectionStateV3::Stopped { .. } => {
                Err(GatewayControlTransitionErrorV3::Stopped)
            }
        }
    }

    fn drain(&mut self) -> Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3> {
        if let GatewayConnectionStateV3::Draining { last_epoch, .. } = self.state {
            return Ok(GatewayCommandAckV3::Draining { last_epoch });
        }
        if matches!(self.state, GatewayConnectionStateV3::Stopped { .. }) {
            return Err(GatewayControlTransitionErrorV3::Stopped);
        }
        let last_epoch = self.state.current_epoch();
        self.publish_transition(
            GatewayConnectionStateV3::Draining {
                last_epoch,
                cause: GatewayDrainCauseV3::Commanded,
            },
            GatewayLifecycleEventV3::Draining {
                last_epoch,
                cause: GatewayDrainCauseV3::Commanded,
            },
            self.admission_snapshot.admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        )?;
        Ok(GatewayCommandAckV3::Draining { last_epoch })
    }

    fn require_running(&self) -> Result<(), GatewayControlTransitionErrorV3> {
        if !self.invalidation.is_healthy() {
            return Err(GatewayControlTransitionErrorV3::ControlOrphaned);
        }
        match self.state {
            GatewayConnectionStateV3::Draining { .. } => {
                Err(GatewayControlTransitionErrorV3::Draining)
            }
            GatewayConnectionStateV3::Stopped { .. } => {
                Err(GatewayControlTransitionErrorV3::Stopped)
            }
            _ => Ok(()),
        }
    }

    fn require_admission_snapshot(
        &self,
        pause_token: &GatewayPauseTokenV3,
    ) -> Result<(), GatewayControlTransitionErrorV3> {
        if self.admission_snapshot.admission_revision == pause_token.admission_revision
            && self.admission_snapshot.transition_sequence == pause_token.transition_sequence
        {
            Ok(())
        } else {
            Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot)
        }
    }

    fn next_admission_revision(
        &mut self,
    ) -> Result<GatewayAdmissionRevisionV3, GatewayControlTransitionErrorV3> {
        let next = self
            .admission_snapshot
            .admission_revision
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(GatewayAdmissionRevisionV3);
        let Some(revision) = next else {
            self.fail_closed(GatewayDrainCauseV3::AdmissionRevisionOverflow);
            return Err(GatewayControlTransitionErrorV3::AdmissionRevisionOverflow);
        };
        Ok(revision)
    }

    fn next_admission_snapshot(
        &mut self,
        state: GatewayConnectionStateV3,
        admission_revision: GatewayAdmissionRevisionV3,
        evidence: GatewayAdmissionEvidenceUpdateV3,
    ) -> Result<GatewayAdmissionSnapshotV3, GatewayControlTransitionErrorV3> {
        let Some(sequence) = self
            .admission_snapshot
            .transition_sequence
            .get()
            .checked_add(1)
            .map(GatewayAdmissionSequenceV3)
        else {
            self.fail_closed_without_lifecycle(GatewayDrainCauseV3::AdmissionSequenceOverflow);
            return Err(GatewayControlTransitionErrorV3::AdmissionSequenceOverflow);
        };
        let (connected_event_sequence, resume_sequence) = match evidence {
            GatewayAdmissionEvidenceUpdateV3::Preserve => (
                self.admission_snapshot.connected_event_sequence,
                self.admission_snapshot.resume_sequence,
            ),
            GatewayAdmissionEvidenceUpdateV3::Connected { admitted } => {
                (Some(sequence), admitted.then_some(sequence))
            }
            GatewayAdmissionEvidenceUpdateV3::Resumed => (
                self.admission_snapshot.connected_event_sequence,
                Some(sequence),
            ),
        };
        Ok(GatewayAdmissionSnapshotV3 {
            connection: state,
            admission_revision,
            transition_sequence: sequence,
            connected_event_sequence,
            resume_sequence,
        })
    }

    fn publish_transition(
        &mut self,
        state: GatewayConnectionStateV3,
        event: GatewayLifecycleEventV3,
        admission_revision: GatewayAdmissionRevisionV3,
        evidence: GatewayAdmissionEvidenceUpdateV3,
    ) -> Result<(), GatewayControlTransitionErrorV3> {
        let snapshot = self.next_admission_snapshot(state, admission_revision, evidence)?;
        let lifecycle = self.lifecycle.clone();
        let result = match lifecycle.try_reserve() {
            Ok(permit) => {
                self.replace_snapshot(snapshot);
                permit.send(event);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                self.replace_failed_transition(snapshot, GatewayDrainCauseV3::LifecycleOverflow);
                Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                self.replace_failed_transition(snapshot, GatewayDrainCauseV3::LifecycleClosed);
                Err(GatewayControlTransitionErrorV3::LifecycleClosed)
            }
        };
        result
    }

    fn fail_closed(&mut self, cause: GatewayDrainCauseV3) {
        let last_epoch = self.state.current_epoch();
        let state = GatewayConnectionStateV3::Draining { last_epoch, cause };
        let event = GatewayLifecycleEventV3::Draining { last_epoch, cause };
        let Ok(snapshot) = self.next_admission_snapshot(
            state,
            self.admission_snapshot.admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        ) else {
            return;
        };
        let lifecycle = self.lifecycle.clone();
        let permit = lifecycle.try_reserve().ok();
        self.replace_snapshot(snapshot);
        if let Some(permit) = permit {
            permit.send(event);
        }
    }

    fn fail_closed_without_lifecycle(&mut self, cause: GatewayDrainCauseV3) {
        let last_epoch = self.state.current_epoch();
        self.replace_without_sequence(GatewayConnectionStateV3::Draining { last_epoch, cause });
    }

    fn replace_without_sequence(&mut self, state: GatewayConnectionStateV3) {
        let snapshot = GatewayAdmissionSnapshotV3 {
            connection: state,
            ..self.admission_snapshot
        };
        self.replace_snapshot(snapshot);
    }

    fn replace_failed_transition(
        &mut self,
        attempted: GatewayAdmissionSnapshotV3,
        cause: GatewayDrainCauseV3,
    ) {
        let snapshot = GatewayAdmissionSnapshotV3 {
            connection: GatewayConnectionStateV3::Draining {
                last_epoch: self.state.current_epoch(),
                cause,
            },
            admission_revision: attempted.admission_revision,
            transition_sequence: attempted.transition_sequence,
            connected_event_sequence: self.admission_snapshot.connected_event_sequence,
            resume_sequence: self.admission_snapshot.resume_sequence,
        };
        self.replace_snapshot(snapshot);
    }

    fn replace_snapshot(&mut self, snapshot: GatewayAdmissionSnapshotV3) {
        if let Some(signal) = invalidation_signal(self.state, snapshot.connection) {
            self.invalidation.invalidate(signal);
        }
        self.state = snapshot.connection;
        self.admission_snapshot = snapshot;
        self.admission.send_replace(snapshot);
        self.connection.send_replace(snapshot.connection);
    }
}

fn invalidation_signal(
    previous: GatewayConnectionStateV3,
    next: GatewayConnectionStateV3,
) -> Option<GatewayInvalidationSignalV3> {
    match next {
        GatewayConnectionStateV3::Disconnected { kind, .. }
        | GatewayConnectionStateV3::Paused {
            connection: GatewayPausedConnectionV3::Disconnected { kind, .. },
        } => Some(GatewayInvalidationSignalV3::Disconnected(kind)),
        GatewayConnectionStateV3::Paused { .. } if previous.admits_interactions() => {
            Some(GatewayInvalidationSignalV3::AdmissionPaused)
        }
        GatewayConnectionStateV3::Draining { cause, .. } => {
            Some(GatewayInvalidationSignalV3::Draining(cause))
        }
        GatewayConnectionStateV3::Stopped { cause, .. } => {
            Some(GatewayInvalidationSignalV3::Stopped(cause))
        }
        GatewayConnectionStateV3::Starting
        | GatewayConnectionStateV3::Connected { .. }
        | GatewayConnectionStateV3::Paused { .. } => None,
    }
}

impl Drop for SharedGatewayRuntimeControlV3 {
    fn drop(&mut self) {
        if matches!(self.state, GatewayConnectionStateV3::Stopped { .. }) {
            return;
        }
        let last_epoch = self.state.current_epoch();
        let cause = match self.state {
            GatewayConnectionStateV3::Draining { cause, .. } => cause,
            _ => GatewayDrainCauseV3::RuntimeFailure,
        };
        let state = GatewayConnectionStateV3::Stopped { last_epoch, cause };
        let lifecycle = self.lifecycle.clone();
        let permit = lifecycle.try_reserve().ok();
        match self.next_admission_snapshot(
            state,
            self.admission_snapshot.admission_revision,
            GatewayAdmissionEvidenceUpdateV3::Preserve,
        ) {
            Ok(snapshot) => {
                self.replace_snapshot(snapshot);
                if let Some(permit) = permit {
                    permit.send(GatewayLifecycleEventV3::Stopped { last_epoch, cause });
                }
            }
            Err(_) => {
                let cause = match self.state {
                    GatewayConnectionStateV3::Draining { cause, .. } => cause,
                    _ => GatewayDrainCauseV3::AdmissionSequenceOverflow,
                };
                self.replace_without_sequence(GatewayConnectionStateV3::Stopped {
                    last_epoch,
                    cause,
                });
            }
        }
    }
}

fn command_outcome(
    result: &Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>,
) -> GatewayRuntimeCommandOutcomeV3 {
    match result {
        Ok(acknowledgement) => GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement.clone()),
        Err(error) => GatewayRuntimeCommandOutcomeV3::Rejected(*error),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingInvalidatorV3 {
        signals: Arc<Mutex<Vec<GatewayInvalidationSignalV3>>>,
    }

    impl GatewaySynchronousInvalidatorV3 for RecordingInvalidatorV3 {
        fn invalidate(&self, signal: GatewayInvalidationSignalV3) {
            self.signals.lock().unwrap().push(signal);
        }
    }

    impl RecordingInvalidatorV3 {
        fn signals(&self) -> Vec<GatewayInvalidationSignalV3> {
            self.signals.lock().unwrap().clone()
        }
    }

    struct BlockingInvalidatorV3 {
        target: GatewayInvalidationSignalV3,
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
    }

    impl GatewaySynchronousInvalidatorV3 for BlockingInvalidatorV3 {
        fn invalidate(&self, signal: GatewayInvalidationSignalV3) {
            if signal == self.target {
                self.entered.wait();
                self.release.wait();
            }
        }
    }

    async fn pause(
        control: &SharedGatewayControlV3,
        runtime: &mut SharedGatewayRuntimeControlV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (result, outcome) =
            tokio::join!(control.pause_admission(), runtime.process_next_command());
        assert!(matches!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::Paused { .. })
        ));
        result
    }

    async fn resume(
        control: &SharedGatewayControlV3,
        runtime: &mut SharedGatewayRuntimeControlV3,
        pause_token: &GatewayPauseTokenV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (result, _) = tokio::join!(
            control.resume_admission(pause_token),
            runtime.process_next_command()
        );
        result
    }

    fn resume_token(acknowledgement: GatewayCommandAckV3) -> GatewayPauseTokenV3 {
        match acknowledgement {
            GatewayCommandAckV3::Paused { resume_token, .. } => resume_token,
            _ => panic!("expected paused acknowledgement"),
        }
    }

    fn queue_resume(
        control: &SharedGatewayControlV3,
        pause_token: &GatewayPauseTokenV3,
    ) -> oneshot::Receiver<Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>> {
        let (sender, receiver) = oneshot::channel();
        control
            .commands
            .try_send(GatewayCommandV3::Resume {
                pause_token: pause_token.clone(),
                acknowledgement: sender,
            })
            .unwrap();
        receiver
    }

    #[test]
    fn disconnect_invalidation_finishes_before_the_closed_snapshot_is_published() {
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            BlockingInvalidatorV3 {
                target: GatewayInvalidationSignalV3::Disconnected(
                    GatewayDisconnectKindV3::Reconnect,
                ),
                entered: entered.clone(),
                release: release.clone(),
            },
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let observer = control.connection_observer();
        let lease = observer.issue_ready_lease(epoch).unwrap();
        let disconnect = thread::spawn(move || {
            let result = runtime.mark_disconnected(GatewayDisconnectKindV3::Reconnect);
            (runtime, result)
        });

        entered.wait();
        assert!(matches!(
            observer.current_connection(),
            GatewayConnectionStateV3::Connected { .. }
        ));
        assert!(observer.ready_lease_is_current(&lease));
        release.wait();

        let (runtime, result) = disconnect.join().unwrap();
        assert_eq!(result, Ok(()));
        assert!(matches!(
            observer.current_connection(),
            GatewayConnectionStateV3::Disconnected {
                kind: GatewayDisconnectKindV3::Reconnect,
                ..
            }
        ));
        assert!(!observer.ready_lease_is_current(&lease));
        drop(runtime);
    }

    #[test]
    fn control_owner_invalidation_finishes_before_liveness_is_closed() {
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            BlockingInvalidatorV3 {
                target: GatewayInvalidationSignalV3::ControlOrphaned,
                entered: entered.clone(),
                release: release.clone(),
            },
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let observer = control.connection_observer();
        let lease = observer.issue_ready_lease(epoch).unwrap();
        let owner_drop = thread::spawn(move || drop(control));

        entered.wait();
        assert!(observer.ready_lease_is_current(&lease));
        release.wait();

        owner_drop.join().unwrap();
        assert!(!observer.ready_lease_is_current(&lease));
    }

    #[tokio::test]
    async fn admission_pause_is_synchronously_invalidated_before_acknowledgement() {
        let invalidator = RecordingInvalidatorV3::default();
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            invalidator.clone(),
        );
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();

        let acknowledgement = pause(&control, &mut runtime).await.unwrap();

        assert!(matches!(
            acknowledgement,
            GatewayCommandAckV3::Paused { .. }
        ));
        assert_eq!(
            invalidator.signals(),
            [GatewayInvalidationSignalV3::AdmissionPaused]
        );
    }

    #[tokio::test]
    async fn explicit_ready_and_resume_do_not_invalidate_an_already_closed_generation() {
        let invalidator = RecordingInvalidatorV3::default();
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
            invalidator.clone(),
        );
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let paused = pause(&control, &mut runtime).await.unwrap();
        let token = resume_token(paused);
        resume(&control, &mut runtime, &token).await.unwrap();

        assert!(invalidator.signals().is_empty());
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Connected { .. }
        ));
    }

    #[test]
    fn every_disconnect_kind_is_preserved_by_the_invalidation_signal() {
        for kind in [
            GatewayDisconnectKindV3::Close,
            GatewayDisconnectKindV3::Reconnect,
            GatewayDisconnectKindV3::SessionInvalidated,
            GatewayDisconnectKindV3::ReceiveError,
        ] {
            let invalidator = RecordingInvalidatorV3::default();
            let (control, mut runtime) =
                shared_gateway_control_channel_with_policy_and_invalidator_v3(
                    GatewayControlConfigV3::default(),
                    GatewayAdmissionPolicyV3::ResumeOnConnect,
                    invalidator.clone(),
                );
            runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();

            runtime.mark_disconnected(kind).unwrap();

            assert_eq!(
                invalidator.signals(),
                [GatewayInvalidationSignalV3::Disconnected(kind)]
            );
            drop(runtime);
            drop(control);
        }
    }

    #[test]
    fn lifecycle_overflow_is_invalidated_before_the_draining_snapshot() {
        let invalidator = RecordingInvalidatorV3::default();
        let config = GatewayControlConfigV3::new(
            NonZeroUsize::new(1).unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            config,
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            invalidator.clone(),
        );

        assert_eq!(
            runtime.mark_connected(GatewayReadyKindV3::Ready),
            Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
        );
        assert_eq!(
            invalidator.signals(),
            [GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::LifecycleOverflow
            )]
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::LifecycleOverflow,
                ..
            }
        ));
    }

    #[test]
    fn runtime_drop_is_invalidated_before_stopped_is_published() {
        let invalidator = RecordingInvalidatorV3::default();
        let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            invalidator.clone(),
        );

        drop(runtime);

        assert_eq!(
            invalidator.signals(),
            [GatewayInvalidationSignalV3::Stopped(
                GatewayDrainCauseV3::RuntimeFailure
            )]
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Stopped {
                cause: GatewayDrainCauseV3::RuntimeFailure,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn commanded_drain_is_invalidated_before_acknowledgement() {
        let invalidator = RecordingInvalidatorV3::default();
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            invalidator.clone(),
        );
        let (acknowledgement, outcome) =
            tokio::join!(control.begin_drain(), runtime.process_next_command());

        assert!(matches!(
            acknowledgement.unwrap(),
            GatewayCommandAckV3::Draining { .. }
        ));
        assert!(matches!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::Draining { .. })
        ));
        assert_eq!(
            invalidator.signals(),
            [GatewayInvalidationSignalV3::Draining(
                GatewayDrainCauseV3::Commanded
            )]
        );
    }

    struct PanickingInvalidatorV3;

    impl GatewaySynchronousInvalidatorV3 for PanickingInvalidatorV3 {
        fn invalidate(&self, _: GatewayInvalidationSignalV3) {
            panic!("invalidation panic")
        }
    }

    #[test]
    fn invalidator_panic_is_contained_and_permanently_closes_ready_leases() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            PanickingInvalidatorV3,
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let observer = control.connection_observer();
        let lease = observer.issue_ready_lease(epoch).unwrap();

        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Close)
            .unwrap();

        assert!(!observer.ready_lease_is_current(&lease));
        assert_eq!(
            runtime.mark_connected(GatewayReadyKindV3::Ready),
            Err(GatewayControlTransitionErrorV3::ControlOrphaned)
        );
        assert_eq!(
            observer.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::ControlOrphaned)
        );
    }

    struct BlockingPanickingInvalidatorV3 {
        calls: Arc<AtomicUsize>,
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
    }

    impl GatewaySynchronousInvalidatorV3 for BlockingPanickingInvalidatorV3 {
        fn invalidate(&self, _: GatewayInvalidationSignalV3) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.wait();
            self.release.wait();
            panic!("serialized invalidation panic")
        }
    }

    #[test]
    fn concurrent_callbacks_are_serial_and_stop_after_the_first_panic() {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let started = Arc::new(Barrier::new(2));
        let (control, runtime) = shared_gateway_control_channel_with_policy_and_invalidator_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ResumeOnConnect,
            BlockingPanickingInvalidatorV3 {
                calls: calls.clone(),
                entered: entered.clone(),
                release: release.clone(),
            },
        );
        let owner_drop = thread::spawn(move || drop(control));
        entered.wait();
        let runtime_started = started.clone();
        let runtime_drop = thread::spawn(move || {
            runtime_started.wait();
            drop(runtime);
        });
        started.wait();
        release.wait();

        owner_drop.join().unwrap();
        runtime_drop.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn command_acknowledgements_follow_applied_state_barriers() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();

        let pause_future = control.pause_admission();
        tokio::pin!(pause_future);
        assert!(
            tokio::time::timeout(Duration::from_millis(1), &mut pause_future)
                .await
                .is_err()
        );
        assert!(matches!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::Paused {
                epoch: Some(value),
                ..
            }) if value == epoch
        ));
        let paused = pause_future.await.unwrap();
        assert!(matches!(
            &paused,
            GatewayCommandAckV3::Paused {
                epoch: Some(value),
                ..
            } if *value == epoch
        ));
        let pause_token = resume_token(paused);
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));

        let (resumed, outcome) = tokio::join!(
            control.resume_admission(&pause_token),
            runtime.process_next_command()
        );
        assert_eq!(resumed, Ok(GatewayCommandAckV3::AdmissionResumed { epoch }));
        assert_eq!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::AdmissionResumed {
                epoch
            })
        );
        assert!(control.current_connection().admits_interactions());

        let (drained, outcome) =
            tokio::join!(control.begin_drain(), runtime.process_next_command());
        assert_eq!(
            drained,
            Ok(GatewayCommandAckV3::Draining {
                last_epoch: Some(epoch)
            })
        );
        assert_eq!(
            outcome,
            GatewayRuntimeCommandOutcomeV3::Applied(GatewayCommandAckV3::Draining {
                last_epoch: Some(epoch)
            })
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Draining { .. }
        ));
    }

    #[tokio::test]
    async fn stale_epoch_and_non_connected_resume_are_rejected() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let first = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let first_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        let second = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        let second_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        assert!(second > first);
        assert_eq!(
            resume(&control, &mut runtime, &first_pause).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        assert_eq!(
            resume(&control, &mut runtime, &second_pause).await,
            Ok(GatewayCommandAckV3::AdmissionResumed { epoch: second })
        );
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Close)
            .unwrap();
        assert_eq!(
            resume(&control, &mut runtime, &second_pause).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::NotConnected
            ))
        );
    }

    #[tokio::test]
    async fn disconnect_pause_and_drain_invalidate_ready_leases() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = control.issue_ready_lease(epoch).unwrap();
        assert!(control.ready_lease_is_current(&lease));
        let pause_token = resume_token(pause(&control, &mut runtime).await.unwrap());
        assert!(!control.ready_lease_is_current(&lease));
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
        resume(&control, &mut runtime, &pause_token).await.unwrap();
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::ReceiveError)
            .unwrap();
        assert!(!control.ready_lease_is_current(&lease));
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::NotConnected)
        );
        let second = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        assert!(second > epoch);
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::StaleConnectionEpoch)
        );
        let second_lease = control.issue_ready_lease(second).unwrap();
        let (_, _) = tokio::join!(control.begin_drain(), runtime.process_next_command());
        assert!(!control.ready_lease_is_current(&second_lease));
        assert_eq!(
            control.issue_ready_lease(second),
            Err(GatewayControlTransitionErrorV3::Draining)
        );
    }

    #[tokio::test]
    async fn orphaned_control_fails_closed_without_waiting() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        drop(control);
        assert_eq!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::ControlOrphaned
        );
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::ControlOrphaned,
                ..
            }
        ));
        runtime.mark_stopped().unwrap_err();
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Stopped { .. }
        ));
    }

    #[tokio::test]
    async fn epoch_and_lifecycle_overflow_fail_closed() {
        let config = GatewayControlConfigV3::new(
            NonZeroUsize::new(1).unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
        let (mut control, mut runtime) = shared_gateway_control_channel_v3(config);
        assert_eq!(
            runtime.mark_connected(GatewayReadyKindV3::Ready),
            Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
        );
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::LifecycleOverflow,
                ..
            }
        ));
        assert_eq!(
            runtime.admission_snapshot.transition_sequence(),
            GatewayAdmissionSequenceV3(1)
        );
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Starting)
        );

        let (mut control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Starting)
        );
        runtime.last_issued_epoch = u64::MAX;
        assert_eq!(
            runtime.mark_connected(GatewayReadyKindV3::Ready),
            Err(GatewayControlTransitionErrorV3::ConnectionEpochOverflow)
        );
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::ConnectionEpochOverflow,
                ..
            }
        ));
        assert!(matches!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Draining {
                cause: GatewayDrainCauseV3::ConnectionEpochOverflow,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn runtime_drop_publishes_stopped_state_to_watchers() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = control.issue_ready_lease(epoch).unwrap();
        drop(runtime);
        assert!(!control.ready_lease_is_current(&lease));
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Stopped {
                cause: GatewayDrainCauseV3::RuntimeFailure,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn lifecycle_history_is_lossless_while_watch_exposes_current_state() {
        let (mut control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let pause_token = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &pause_token).await.unwrap();
        let (_, _) = tokio::join!(control.begin_drain(), runtime.process_next_command());
        runtime.mark_stopped().unwrap();

        let events = [
            GatewayLifecycleEventV3::Starting,
            GatewayLifecycleEventV3::Connected {
                epoch,
                kind: GatewayReadyKindV3::Ready,
                paused: false,
            },
            GatewayLifecycleEventV3::Paused { epoch: Some(epoch) },
            GatewayLifecycleEventV3::AdmissionResumed { epoch },
            GatewayLifecycleEventV3::Draining {
                last_epoch: Some(epoch),
                cause: GatewayDrainCauseV3::Commanded,
            },
            GatewayLifecycleEventV3::Stopped {
                last_epoch: Some(epoch),
                cause: GatewayDrainCauseV3::Commanded,
            },
        ];
        for expected in events {
            assert_eq!(control.next_lifecycle().await, Some(expected));
        }
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Stopped {
                last_epoch: Some(value),
                cause: GatewayDrainCauseV3::Commanded
            } if value == epoch
        ));
    }

    #[tokio::test]
    async fn lifecycle_delivery_observes_the_already_published_snapshot() {
        let (mut control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Starting)
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        assert!(matches!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Connected {
                epoch: value,
                paused: false,
                ..
            }) if value == epoch
        ));
        assert!(control.current_connection().admits_interactions());

        runtime.pause().unwrap();
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV3::Paused { epoch: Some(epoch) })
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));
    }

    #[tokio::test]
    async fn cloned_observer_issues_and_invalidates_leases_without_command_ownership() {
        let (_control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let observer = runtime.connection_observer();
        let mut second = observer.clone();
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        assert_eq!(
            second.connection_changed().await,
            Some(GatewayConnectionStateV3::Connected {
                epoch,
                kind: GatewayReadyKindV3::Ready
            })
        );
        let lease = observer.issue_ready_lease(epoch).unwrap();
        assert!(second.ready_lease_is_current(&lease));
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        assert!(!observer.ready_lease_is_current(&lease));
    }

    #[tokio::test]
    async fn pause_resume_never_revalidates_a_pre_pause_lease() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let observer = control.connection_observer();
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let before_pause = observer.issue_ready_lease(epoch).unwrap();
        let pause_token = resume_token(pause(&control, &mut runtime).await.unwrap());
        assert!(!observer.ready_lease_is_current(&before_pause));
        resume(&control, &mut runtime, &pause_token).await.unwrap();
        assert!(!observer.ready_lease_is_current(&before_pause));
        let after_resume = observer.issue_ready_lease(epoch).unwrap();
        assert_eq!(after_resume.epoch(), before_pause.epoch());
        assert!(after_resume.admission_revision() > before_pause.admission_revision());
        assert!(observer.ready_lease_is_current(&after_resume));
    }

    #[tokio::test]
    async fn every_acknowledged_pause_fences_an_older_resume_command() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let first_token = resume_token(runtime.pause().unwrap());
        let first_pause = control.current_admission_snapshot();
        let stale = queue_resume(&control, &first_token);

        runtime.pause().unwrap();
        let second_pause = control.current_admission_snapshot();
        assert!(second_pause.admission_revision() > first_pause.admission_revision());
        assert!(second_pause.transition_sequence() > first_pause.transition_sequence());
        assert_eq!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::Rejected(
                GatewayControlTransitionErrorV3::StaleAdmissionSnapshot
            )
        );
        assert_eq!(
            stale.await,
            Ok(Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot))
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));
    }

    #[tokio::test]
    async fn delayed_older_workflow_cannot_resume_a_newer_pause() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let older = resume_token(pause(&control, &mut runtime).await.unwrap());
        let newer = resume_token(pause(&control, &mut runtime).await.unwrap());

        assert_eq!(
            resume(&control, &mut runtime, &older).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleAdmissionSnapshot
            ))
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));
        resume(&control, &mut runtime, &newer).await.unwrap();
        assert!(control.current_connection().admits_interactions());
    }

    #[tokio::test]
    async fn foreign_pause_token_is_rejected_before_and_inside_the_runtime() {
        let (control_a, mut runtime_a) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        runtime_a.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let token_a = resume_token(runtime_a.pause().unwrap());

        let (control_b, mut runtime_b) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        runtime_b.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        runtime_b.pause().unwrap();
        assert_eq!(
            control_b.resume_admission(&token_a).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleAdmissionSnapshot
            ))
        );

        let acknowledgement = queue_resume(&control_b, &token_a);
        assert_eq!(
            runtime_b.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::Rejected(
                GatewayControlTransitionErrorV3::StaleAdmissionSnapshot
            )
        );
        assert_eq!(
            acknowledgement.await,
            Ok(Err(GatewayControlTransitionErrorV3::StaleAdmissionSnapshot))
        );
        assert!(matches!(
            control_b.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));
        drop(control_a);
    }

    #[tokio::test]
    async fn buffered_resume_cannot_outlive_the_control_owner() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let observer = control.connection_observer();
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = observer.issue_ready_lease(epoch).unwrap();
        let pause_token = resume_token(runtime.pause().unwrap());
        let acknowledgement = queue_resume(&control, &pause_token);

        drop(control);
        assert!(!observer.ready_lease_is_current(&lease));
        assert_eq!(
            observer.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::ControlOrphaned)
        );
        assert_eq!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::ControlOrphaned
        );
        assert!(acknowledgement.await.is_err());
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::ControlOrphaned,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn resume_cancelled_before_runtime_claim_never_opens_admission() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let pause_token = resume_token(runtime.pause().unwrap());
        let acknowledgement = queue_resume(&control, &pause_token);
        drop(acknowledgement);

        assert_eq!(
            runtime.process_next_command().await,
            GatewayRuntimeCommandOutcomeV3::Rejected(
                GatewayControlTransitionErrorV3::StaleAdmissionSnapshot
            )
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
    }

    #[tokio::test]
    async fn explicit_resume_policy_starts_paused_and_stays_paused_after_reconnect() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        assert_eq!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Starting,
            }
        );
        let first = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        assert_eq!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected {
                    epoch: first,
                    kind: GatewayReadyKindV3::Ready,
                },
            }
        );
        assert_eq!(
            control.issue_ready_lease(first),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
        let first_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &first_pause).await.unwrap();
        let first_lease = control.issue_ready_lease(first).unwrap();
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        assert!(!control.ready_lease_is_current(&first_lease));
        assert_eq!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Disconnected {
                    last_epoch: Some(first),
                    kind: GatewayDisconnectKindV3::Reconnect,
                },
            }
        );
        let second = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        assert!(second > first);
        assert_eq!(
            control.issue_ready_lease(second),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
        let second_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        assert_eq!(
            resume(&control, &mut runtime, &first_pause).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        resume(&control, &mut runtime, &second_pause).await.unwrap();
        let second_lease = control.issue_ready_lease(second).unwrap();
        assert!(second_lease.admission_revision() > first_lease.admission_revision());
        assert!(control.ready_lease_is_current(&second_lease));
    }

    #[tokio::test]
    async fn explicit_disconnect_revision_overflow_fails_closed() {
        let (_control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        runtime.admission_snapshot.admission_revision =
            GatewayAdmissionRevisionV3(NonZeroU64::new(u64::MAX).unwrap());
        assert_eq!(
            runtime.mark_disconnected(GatewayDisconnectKindV3::ReceiveError),
            Err(GatewayControlTransitionErrorV3::AdmissionRevisionOverflow)
        );
        assert!(matches!(
            runtime.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::AdmissionRevisionOverflow,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn explicit_policy_repauses_even_without_a_disconnect_transition() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        let first = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let first_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &first_pause).await.unwrap();
        let first_lease = control.issue_ready_lease(first).unwrap();
        let second = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        assert!(second > first);
        assert!(!control.ready_lease_is_current(&first_lease));
        assert_eq!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected {
                    epoch: second,
                    kind: GatewayReadyKindV3::Resumed,
                },
            }
        );
        assert_eq!(
            control.issue_ready_lease(second),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
        let second_pause = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &second_pause).await.unwrap();
        assert!(control.ready_lease_is_current(&control.issue_ready_lease(second).unwrap()));
    }

    #[tokio::test]
    async fn explicit_disconnect_publishes_one_closed_revision_snapshot() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let pause_token = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &pause_token).await.unwrap();
        let before = control.current_admission_snapshot();
        let lease = control.issue_ready_lease(epoch).unwrap();

        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();

        let after = control.current_admission_snapshot();
        assert!(matches!(
            after.connection(),
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Disconnected {
                    last_epoch: Some(value),
                    kind: GatewayDisconnectKindV3::Reconnect,
                },
            } if value == epoch
        ));
        assert!(after.admission_revision() > before.admission_revision());
        assert!(after.transition_sequence() > before.transition_sequence());
        assert!(!control.ready_lease_is_current(&lease));
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
    }

    #[tokio::test]
    async fn explicit_ready_lease_binds_the_exact_connect_and_resume_events() {
        let (control, mut runtime) = shared_gateway_control_channel_with_policy_v3(
            GatewayControlConfigV3::default(),
            GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
        );
        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let connected = control.current_admission_snapshot();
        assert!(connected.connected_event_sequence().is_some());
        assert_eq!(connected.resume_sequence(), None);
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );

        let pause_token = resume_token(pause(&control, &mut runtime).await.unwrap());
        resume(&control, &mut runtime, &pause_token).await.unwrap();
        let resumed = control.current_admission_snapshot();
        let lease = control.issue_ready_lease(epoch).unwrap();
        assert_eq!(
            Some(lease.connected_event_sequence()),
            resumed.connected_event_sequence()
        );
        assert_eq!(Some(lease.resume_sequence()), resumed.resume_sequence());
        assert!(lease.resume_sequence() > lease.connected_event_sequence());
        assert!(control.ready_lease_is_current(&lease));
    }

    #[test]
    fn admission_sequence_overflow_fails_closed() {
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        runtime.admission_snapshot.transition_sequence = GatewayAdmissionSequenceV3(u64::MAX);
        assert_eq!(
            runtime.mark_connected(GatewayReadyKindV3::Ready),
            Err(GatewayControlTransitionErrorV3::AdmissionSequenceOverflow)
        );
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Draining {
                cause: GatewayDrainCauseV3::AdmissionSequenceOverflow,
                ..
            }
        ));
    }
}
