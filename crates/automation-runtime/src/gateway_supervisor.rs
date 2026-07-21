use std::future::Future;
use std::time::Duration;

use automation_core::RunningRuleSetIdentity;
use automation_instance::{InstanceIdGenerator, InstanceStore};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset::RuleSetStore;
use automation_ruleset_dispatch::GuildRoleSnapshotProvider;
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;
use twilight_model::gateway::CloseFrame;

use crate::mutation::TwilightMutationAdapter;
use crate::runner::handle_interaction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayCommandV2 {
    DrainAndShutdown { deadline: Duration },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayReadyKindV2 {
    Ready,
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayDisconnectKindV2 {
    Close,
    Reconnect,
    SessionInvalidated,
    ReceiveError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayDrainOutcomeV2 {
    Clean,
    DeadlineExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayLifecycleEventV2 {
    Starting,
    Connected { kind: GatewayReadyKindV2 },
    Disconnected { kind: GatewayDisconnectKindV2 },
    InteractionStarted,
    InteractionFinished,
    Draining,
    Drained { outcome: GatewayDrainOutcomeV2 },
    StreamEnded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayConnectionStateV2 {
    Starting,
    Connected { kind: GatewayReadyKindV2 },
    Disconnected { kind: GatewayDisconnectKindV2 },
    Draining,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayExitV2 {
    Drained { outcome: GatewayDrainOutcomeV2 },
    StreamEnded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GatewayControlErrorV2 {
    #[error("gateway runtime is no longer accepting commands")]
    RuntimeStopped,
}

pub struct GatewayControlV2 {
    commands: tokio::sync::mpsc::Sender<GatewayCommandV2>,
    lifecycle: tokio::sync::mpsc::Receiver<GatewayLifecycleEventV2>,
    connection: tokio::sync::watch::Receiver<GatewayConnectionStateV2>,
}

impl GatewayControlV2 {
    pub async fn drain_and_shutdown(
        &self,
        deadline: Duration,
    ) -> Result<(), GatewayControlErrorV2> {
        self.commands
            .send(GatewayCommandV2::DrainAndShutdown { deadline })
            .await
            .map_err(|_| GatewayControlErrorV2::RuntimeStopped)
    }

    pub async fn next_lifecycle(&mut self) -> Option<GatewayLifecycleEventV2> {
        self.lifecycle.recv().await
    }

    pub fn current_connection(&self) -> GatewayConnectionStateV2 {
        *self.connection.borrow()
    }

    pub async fn connection_changed(&mut self) -> Option<GatewayConnectionStateV2> {
        self.connection
            .changed()
            .await
            .ok()
            .map(|()| *self.connection.borrow_and_update())
    }
}

pub struct GatewayRuntimeControlV2 {
    commands: tokio::sync::mpsc::Receiver<GatewayCommandV2>,
    lifecycle: tokio::sync::mpsc::Sender<GatewayLifecycleEventV2>,
    connection: tokio::sync::watch::Sender<GatewayConnectionStateV2>,
    orphaned_control_drain_timeout: Duration,
}

pub fn control_channel_v2() -> (GatewayControlV2, GatewayRuntimeControlV2) {
    let (command_sender, command_receiver) = tokio::sync::mpsc::channel(4);
    let (lifecycle_sender, lifecycle_receiver) = tokio::sync::mpsc::channel(64);
    let (connection_sender, connection_receiver) =
        tokio::sync::watch::channel(GatewayConnectionStateV2::Starting);
    (
        GatewayControlV2 {
            commands: command_sender,
            lifecycle: lifecycle_receiver,
            connection: connection_receiver,
        },
        GatewayRuntimeControlV2 {
            commands: command_receiver,
            lifecycle: lifecycle_sender,
            connection: connection_sender,
            orphaned_control_drain_timeout: Duration::from_secs(15),
        },
    )
}

enum InteractionBoundaryV2 {
    Completed,
    Drain(GatewayDrainOutcomeV2),
}

async fn wait_for_interaction_or_drain<F>(
    interaction: F,
    control: &mut GatewayRuntimeControlV2,
) -> InteractionBoundaryV2
where
    F: Future<Output = ()>,
{
    tokio::pin!(interaction);
    tokio::select! {
        () = &mut interaction => InteractionBoundaryV2::Completed,
        command = control.commands.recv() => {
            let deadline = match command {
                Some(GatewayCommandV2::DrainAndShutdown { deadline }) => deadline,
                None => control.orphaned_control_drain_timeout,
            };
            let outcome = if tokio::time::timeout(deadline, &mut interaction).await.is_ok() {
                GatewayDrainOutcomeV2::Clean
            } else {
                GatewayDrainOutcomeV2::DeadlineExceeded
            };
            InteractionBoundaryV2::Drain(outcome)
        }
    }
}

async fn emit(control: &GatewayRuntimeControlV2, event: GatewayLifecycleEventV2) {
    let _ = control.lifecycle.send(event).await;
}

async fn finish_drain(
    shard: &mut Shard,
    control: &GatewayRuntimeControlV2,
    outcome: GatewayDrainOutcomeV2,
) -> GatewayExitV2 {
    control
        .connection
        .send_replace(GatewayConnectionStateV2::Draining);
    emit(control, GatewayLifecycleEventV2::Draining).await;
    shard.close(CloseFrame::NORMAL);
    control
        .connection
        .send_replace(GatewayConnectionStateV2::Stopped);
    emit(control, GatewayLifecycleEventV2::Drained { outcome }).await;
    GatewayExitV2::Drained { outcome }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_until_shutdown(
    token: String,
    identity: RunningRuleSetIdentity,
    ruleset: InteractionRuleSet,
    bindings: ResourceBindingMap,
    failure_message: String,
    instances: impl InstanceStore,
    instance_ids: impl InstanceIdGenerator,
    teardown: impl InstanceTeardownService,
    ruleset_store: &impl RuleSetStore,
    snapshot_provider: &impl GuildRoleSnapshotProvider,
    mut control: GatewayRuntimeControlV2,
) -> GatewayExitV2 {
    let http = Client::new(token.clone());
    let mutation = TwilightMutationAdapter::new(&http, identity.key.clone());
    let mut shard = Shard::new(ShardId::ONE, token, Intents::empty());
    let event_types = EventTypeFlags::INTERACTION_CREATE
        | EventTypeFlags::READY
        | EventTypeFlags::RESUMED
        | EventTypeFlags::GATEWAY_RECONNECT
        | EventTypeFlags::GATEWAY_INVALIDATE_SESSION;
    emit(&control, GatewayLifecycleEventV2::Starting).await;

    loop {
        tokio::select! {
            biased;
            command = control.commands.recv() => {
                let outcome = match command {
                    Some(GatewayCommandV2::DrainAndShutdown { .. }) | None => {
                        GatewayDrainOutcomeV2::Clean
                    }
                };
                return finish_drain(&mut shard, &control, outcome).await;
            }
            item = shard.next_event(event_types) => {
                let Some(item) = item else {
                    control.connection.send_replace(GatewayConnectionStateV2::Stopped);
                    emit(&control, GatewayLifecycleEventV2::StreamEnded).await;
                    return GatewayExitV2::StreamEnded;
                };
                let event = match item {
                    Ok(event) => event,
                    Err(_) => {
                        let kind = GatewayDisconnectKindV2::ReceiveError;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Disconnected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Disconnected { kind }).await;
                        continue;
                    }
                };
                match event {
                    Event::Ready(_) => {
                        let kind = GatewayReadyKindV2::Ready;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Connected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Connected { kind }).await;
                    }
                    Event::Resumed => {
                        let kind = GatewayReadyKindV2::Resumed;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Connected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Connected { kind }).await;
                    }
                    Event::GatewayClose(_) => {
                        let kind = GatewayDisconnectKindV2::Close;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Disconnected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Disconnected { kind }).await;
                    }
                    Event::GatewayReconnect => {
                        let kind = GatewayDisconnectKindV2::Reconnect;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Disconnected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Disconnected { kind }).await;
                    }
                    Event::GatewayInvalidateSession(_) => {
                        let kind = GatewayDisconnectKindV2::SessionInvalidated;
                        control.connection.send_replace(
                            GatewayConnectionStateV2::Disconnected { kind },
                        );
                        emit(&control, GatewayLifecycleEventV2::Disconnected { kind }).await;
                    }
                    Event::InteractionCreate(interaction_create) => {
                        emit(&control, GatewayLifecycleEventV2::InteractionStarted).await;
                        let interaction = handle_interaction(
                            &http,
                            &identity,
                            &mutation,
                            &ruleset,
                            &bindings,
                            &interaction_create.0,
                            &failure_message,
                            &instances,
                            &instance_ids,
                            &teardown,
                            ruleset_store,
                            snapshot_provider,
                        );
                        match wait_for_interaction_or_drain(interaction, &mut control).await {
                            InteractionBoundaryV2::Completed => {
                                emit(&control, GatewayLifecycleEventV2::InteractionFinished).await;
                            }
                            InteractionBoundaryV2::Drain(outcome) => {
                                return finish_drain(&mut shard, &control, outcome).await;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lifecycle_events_are_lossless_and_ordered() {
        let (mut control, runtime) = control_channel_v2();
        emit(&runtime, GatewayLifecycleEventV2::Starting).await;
        emit(
            &runtime,
            GatewayLifecycleEventV2::Connected {
                kind: GatewayReadyKindV2::Ready,
            },
        )
        .await;
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV2::Starting)
        );
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV2::Connected {
                kind: GatewayReadyKindV2::Ready
            })
        );
    }

    #[tokio::test]
    async fn pending_interaction_is_bounded_by_drain_deadline() {
        let (control, mut runtime) = control_channel_v2();
        control
            .drain_and_shutdown(Duration::from_millis(1))
            .await
            .unwrap();
        let outcome = wait_for_interaction_or_drain(std::future::pending(), &mut runtime).await;
        assert!(matches!(
            outcome,
            InteractionBoundaryV2::Drain(GatewayDrainOutcomeV2::DeadlineExceeded)
        ));
    }

    #[tokio::test]
    async fn accepted_interaction_can_finish_inside_drain_deadline() {
        let (control, mut runtime) = control_channel_v2();
        let (finish, wait) = tokio::sync::oneshot::channel();
        control
            .drain_and_shutdown(Duration::from_secs(1))
            .await
            .unwrap();
        finish.send(()).unwrap();
        let outcome = wait_for_interaction_or_drain(
            async move {
                let _ = wait.await;
            },
            &mut runtime,
        )
        .await;
        assert!(matches!(
            outcome,
            InteractionBoundaryV2::Drain(GatewayDrainOutcomeV2::Clean)
                | InteractionBoundaryV2::Completed
        ));
    }

    #[tokio::test]
    async fn connection_state_is_observable_without_consuming_events() {
        let (control, runtime) = control_channel_v2();
        runtime
            .connection
            .send_replace(GatewayConnectionStateV2::Connected {
                kind: GatewayReadyKindV2::Resumed,
            });
        assert_eq!(
            control.current_connection(),
            GatewayConnectionStateV2::Connected {
                kind: GatewayReadyKindV2::Resumed
            }
        );
    }
}
