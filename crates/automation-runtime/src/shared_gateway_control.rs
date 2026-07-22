use std::num::{NonZeroU64, NonZeroUsize};

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
    LifecycleOverflow,
    LifecycleClosed,
    RuntimeFailure,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayCommandAckV3 {
    Paused {
        epoch: Option<GatewayConnectionEpochV3>,
    },
    AdmissionResumed {
        epoch: GatewayConnectionEpochV3,
    },
    Draining {
        last_epoch: Option<GatewayConnectionEpochV3>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

enum GatewayCommandV3 {
    Pause {
        acknowledgement:
            oneshot::Sender<Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>>,
    },
    Resume {
        expected_epoch: GatewayConnectionEpochV3,
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
    admission_revision: watch::Receiver<GatewayAdmissionRevisionV3>,
}

#[derive(Clone)]
pub struct GatewayConnectionObserverV3 {
    connection: watch::Receiver<GatewayConnectionStateV3>,
    admission_revision: watch::Receiver<GatewayAdmissionRevisionV3>,
}

impl GatewayConnectionObserverV3 {
    pub fn current_connection(&self) -> GatewayConnectionStateV3 {
        *self.connection.borrow()
    }

    pub async fn connection_changed(&mut self) -> Option<GatewayConnectionStateV3> {
        self.connection
            .changed()
            .await
            .ok()
            .map(|()| *self.connection.borrow_and_update())
    }

    pub fn issue_ready_lease(
        &self,
        expected_epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayReadyLeaseV3, GatewayControlTransitionErrorV3> {
        issue_ready_lease(
            self.current_connection(),
            *self.admission_revision.borrow(),
            expected_epoch,
        )
    }

    pub fn ready_lease_is_current(&self, lease: &GatewayReadyLeaseV3) -> bool {
        ready_lease_is_current(
            self.current_connection(),
            *self.admission_revision.borrow(),
            lease,
        )
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
        expected_epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(GatewayCommandV3::Resume {
                expected_epoch,
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
        *self.connection.borrow()
    }

    pub fn connection_watch(&self) -> watch::Receiver<GatewayConnectionStateV3> {
        self.connection.clone()
    }

    pub fn connection_observer(&self) -> GatewayConnectionObserverV3 {
        GatewayConnectionObserverV3 {
            connection: self.connection.clone(),
            admission_revision: self.admission_revision.clone(),
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
        issue_ready_lease(
            self.current_connection(),
            *self.admission_revision.borrow(),
            expected_epoch,
        )
    }

    pub fn ready_lease_is_current(&self, lease: &GatewayReadyLeaseV3) -> bool {
        ready_lease_is_current(
            self.current_connection(),
            *self.admission_revision.borrow(),
            lease,
        )
    }
}

fn issue_ready_lease(
    state: GatewayConnectionStateV3,
    admission_revision: GatewayAdmissionRevisionV3,
    expected_epoch: GatewayConnectionEpochV3,
) -> Result<GatewayReadyLeaseV3, GatewayControlTransitionErrorV3> {
    match state {
        GatewayConnectionStateV3::Connected { epoch, kind } if epoch == expected_epoch => {
            Ok(GatewayReadyLeaseV3 {
                epoch,
                kind,
                admission_revision,
            })
        }
        GatewayConnectionStateV3::Connected { .. } => {
            Err(GatewayControlTransitionErrorV3::StaleConnectionEpoch)
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
    state: GatewayConnectionStateV3,
    admission_revision: GatewayAdmissionRevisionV3,
    lease: &GatewayReadyLeaseV3,
) -> bool {
    matches!(
        state,
        GatewayConnectionStateV3::Connected { epoch, kind }
            if epoch == lease.epoch
                && kind == lease.kind
                && admission_revision == lease.admission_revision
    )
}

pub struct SharedGatewayRuntimeControlV3 {
    commands: mpsc::Receiver<GatewayCommandV3>,
    lifecycle: mpsc::Sender<GatewayLifecycleEventV3>,
    connection: watch::Sender<GatewayConnectionStateV3>,
    admission_revision: watch::Sender<GatewayAdmissionRevisionV3>,
    state: GatewayConnectionStateV3,
    last_issued_epoch: u64,
    last_admission_revision: u64,
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
    let (admission_revision_sender, admission_revision_receiver) =
        watch::channel(initial_admission_revision);
    lifecycle_sender
        .try_send(GatewayLifecycleEventV3::Starting)
        .expect("validated lifecycle capacity accepts the initial state");
    (
        SharedGatewayControlV3 {
            commands: command_sender,
            lifecycle: lifecycle_receiver,
            connection: connection_receiver,
            admission_revision: admission_revision_receiver,
        },
        SharedGatewayRuntimeControlV3 {
            commands: command_receiver,
            lifecycle: lifecycle_sender,
            connection: connection_sender,
            admission_revision: admission_revision_sender,
            state: initial,
            last_issued_epoch: 0,
            last_admission_revision: initial_admission_revision.get(),
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
            connection: self.connection.subscribe(),
            admission_revision: self.admission_revision.subscribe(),
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
        self.publish(
            state,
            GatewayLifecycleEventV3::Connected {
                epoch,
                kind,
                paused,
            },
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
        self.publish(
            state,
            GatewayLifecycleEventV3::Disconnected {
                last_epoch,
                kind,
                paused,
            },
        )?;
        if explicit_resume {
            self.advance_admission_revision()?;
        }
        Ok(())
    }

    pub async fn process_next_command(&mut self) -> GatewayRuntimeCommandOutcomeV3 {
        let Some(command) = self.commands.recv().await else {
            self.fail_closed(GatewayDrainCauseV3::ControlOrphaned);
            return GatewayRuntimeCommandOutcomeV3::ControlOrphaned;
        };
        match command {
            GatewayCommandV3::Pause { acknowledgement } => {
                let result = self.pause();
                let outcome = command_outcome(result);
                let _ = acknowledgement.send(result);
                outcome
            }
            GatewayCommandV3::Resume {
                expected_epoch,
                acknowledgement,
            } => {
                let result = self.resume(expected_epoch);
                let outcome = command_outcome(result);
                let _ = acknowledgement.send(result);
                outcome
            }
            GatewayCommandV3::Drain { acknowledgement } => {
                let result = self.drain();
                let outcome = command_outcome(result);
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
        match self.lifecycle.try_send(event) {
            Ok(()) => {
                self.replace_state(state);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.replace_state(state);
                Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.replace_state(state);
                Err(GatewayControlTransitionErrorV3::LifecycleClosed)
            }
        }
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
            GatewayConnectionStateV3::Paused { connection } => {
                return Ok(GatewayCommandAckV3::Paused {
                    epoch: connection.current_epoch(),
                });
            }
            GatewayConnectionStateV3::Draining { .. } => {
                return Err(GatewayControlTransitionErrorV3::Draining)
            }
            GatewayConnectionStateV3::Stopped { .. } => {
                return Err(GatewayControlTransitionErrorV3::Stopped)
            }
        };
        let epoch = connection.current_epoch();
        self.publish(
            GatewayConnectionStateV3::Paused { connection },
            GatewayLifecycleEventV3::Paused { epoch },
        )?;
        self.advance_admission_revision()?;
        Ok(GatewayCommandAckV3::Paused { epoch })
    }

    fn resume(
        &mut self,
        expected_epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3> {
        match self.state {
            GatewayConnectionStateV3::Paused {
                connection: GatewayPausedConnectionV3::Connected { epoch, kind },
            } if epoch == expected_epoch => {
                self.publish(
                    GatewayConnectionStateV3::Connected { epoch, kind },
                    GatewayLifecycleEventV3::AdmissionResumed { epoch },
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
        self.publish(
            GatewayConnectionStateV3::Draining {
                last_epoch,
                cause: GatewayDrainCauseV3::Commanded,
            },
            GatewayLifecycleEventV3::Draining {
                last_epoch,
                cause: GatewayDrainCauseV3::Commanded,
            },
        )?;
        Ok(GatewayCommandAckV3::Draining { last_epoch })
    }

    fn require_running(&self) -> Result<(), GatewayControlTransitionErrorV3> {
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

    fn advance_admission_revision(
        &mut self,
    ) -> Result<GatewayAdmissionRevisionV3, GatewayControlTransitionErrorV3> {
        let next = self
            .last_admission_revision
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .map(GatewayAdmissionRevisionV3);
        let Some(revision) = next else {
            self.fail_closed(GatewayDrainCauseV3::AdmissionRevisionOverflow);
            return Err(GatewayControlTransitionErrorV3::AdmissionRevisionOverflow);
        };
        self.last_admission_revision = revision.get();
        self.admission_revision.send_replace(revision);
        Ok(revision)
    }

    fn publish(
        &mut self,
        state: GatewayConnectionStateV3,
        event: GatewayLifecycleEventV3,
    ) -> Result<(), GatewayControlTransitionErrorV3> {
        match self.lifecycle.try_send(event) {
            Ok(()) => {
                self.replace_state(state);
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.fail_closed_without_lifecycle(GatewayDrainCauseV3::LifecycleOverflow);
                Err(GatewayControlTransitionErrorV3::LifecycleOverflow)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.fail_closed_without_lifecycle(GatewayDrainCauseV3::LifecycleClosed);
                Err(GatewayControlTransitionErrorV3::LifecycleClosed)
            }
        }
    }

    fn fail_closed(&mut self, cause: GatewayDrainCauseV3) {
        let last_epoch = self.state.current_epoch();
        let state = GatewayConnectionStateV3::Draining { last_epoch, cause };
        let event = GatewayLifecycleEventV3::Draining { last_epoch, cause };
        if self.lifecycle.try_send(event).is_ok() {
            self.replace_state(state);
        } else {
            self.fail_closed_without_lifecycle(cause);
        }
    }

    fn fail_closed_without_lifecycle(&mut self, cause: GatewayDrainCauseV3) {
        let last_epoch = self.state.current_epoch();
        self.replace_state(GatewayConnectionStateV3::Draining { last_epoch, cause });
    }

    fn replace_state(&mut self, state: GatewayConnectionStateV3) {
        self.state = state;
        self.connection.send_replace(state);
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
        let _ = self
            .lifecycle
            .try_send(GatewayLifecycleEventV3::Stopped { last_epoch, cause });
        self.replace_state(state);
    }
}

fn command_outcome(
    result: Result<GatewayCommandAckV3, GatewayControlTransitionErrorV3>,
) -> GatewayRuntimeCommandOutcomeV3 {
    match result {
        Ok(acknowledgement) => GatewayRuntimeCommandOutcomeV3::Applied(acknowledgement),
        Err(error) => GatewayRuntimeCommandOutcomeV3::Rejected(error),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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
        epoch: GatewayConnectionEpochV3,
    ) -> Result<GatewayCommandAckV3, GatewayControlErrorV3> {
        let (result, _) = tokio::join!(
            control.resume_admission(epoch),
            runtime.process_next_command()
        );
        result
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
                epoch: Some(value)
            }) if value == epoch
        ));
        assert!(matches!(
            pause_future.await,
            Ok(GatewayCommandAckV3::Paused { epoch: Some(value) }) if value == epoch
        ));
        assert!(matches!(
            control.current_connection(),
            GatewayConnectionStateV3::Paused { .. }
        ));

        let (resumed, outcome) = tokio::join!(
            control.resume_admission(epoch),
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
        pause(&control, &mut runtime).await.unwrap();
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Reconnect)
            .unwrap();
        let second = runtime.mark_connected(GatewayReadyKindV3::Resumed).unwrap();
        assert!(second > first);
        assert_eq!(
            resume(&control, &mut runtime, first).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        assert_eq!(
            resume(&control, &mut runtime, second).await,
            Ok(GatewayCommandAckV3::AdmissionResumed { epoch: second })
        );
        runtime
            .mark_disconnected(GatewayDisconnectKindV3::Close)
            .unwrap();
        assert_eq!(
            resume(&control, &mut runtime, second).await,
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
        pause(&control, &mut runtime).await.unwrap();
        assert!(!control.ready_lease_is_current(&lease));
        assert_eq!(
            control.issue_ready_lease(epoch),
            Err(GatewayControlTransitionErrorV3::AdmissionPaused)
        );
        resume(&control, &mut runtime, epoch).await.unwrap();
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
        pause(&control, &mut runtime).await.unwrap();
        resume(&control, &mut runtime, epoch).await.unwrap();
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
        pause(&control, &mut runtime).await.unwrap();
        assert!(!observer.ready_lease_is_current(&before_pause));
        resume(&control, &mut runtime, epoch).await.unwrap();
        assert!(!observer.ready_lease_is_current(&before_pause));
        let after_resume = observer.issue_ready_lease(epoch).unwrap();
        assert_eq!(after_resume.epoch(), before_pause.epoch());
        assert!(after_resume.admission_revision() > before_pause.admission_revision());
        assert!(observer.ready_lease_is_current(&after_resume));
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
        resume(&control, &mut runtime, first).await.unwrap();
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
        assert_eq!(
            resume(&control, &mut runtime, first).await,
            Err(GatewayControlErrorV3::Transition(
                GatewayControlTransitionErrorV3::StaleConnectionEpoch
            ))
        );
        resume(&control, &mut runtime, second).await.unwrap();
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
        runtime.last_admission_revision = u64::MAX;
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
        resume(&control, &mut runtime, first).await.unwrap();
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
        resume(&control, &mut runtime, second).await.unwrap();
        assert!(control.ready_lease_is_current(&control.issue_ready_lease(second).unwrap()));
    }

    #[test]
    fn explicit_disconnect_closes_admission_before_advancing_revision() {
        let source = include_str!("shared_gateway_control.rs");
        let method = source
            .split("pub fn mark_disconnected(")
            .nth(1)
            .and_then(|tail| tail.split("pub async fn process_next_command").next())
            .unwrap();
        let publish = method.find("self.publish(").unwrap();
        let advance = method.find("self.advance_admission_revision()?").unwrap();
        assert!(publish < advance);
    }
}
