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
pub enum GatewayCommandV1 {
    Serve,
    DrainAndShutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayReadyKindV1 {
    Ready,
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayDisconnectKindV1 {
    Close,
    Reconnect,
    SessionInvalidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayLifecycleEventV1 {
    Starting,
    Serving { kind: GatewayReadyKindV1 },
    Disconnected { kind: GatewayDisconnectKindV1 },
    ReceiveError,
    Draining,
    Drained,
    StreamEnded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayExitV1 {
    Drained,
    StreamEnded,
}

pub struct GatewayControlV1 {
    commands: tokio::sync::watch::Sender<GatewayCommandV1>,
    lifecycle: tokio::sync::watch::Receiver<GatewayLifecycleEventV1>,
}

impl GatewayControlV1 {
    pub fn drain_and_shutdown(&self) -> bool {
        self.commands
            .send(GatewayCommandV1::DrainAndShutdown)
            .is_ok()
    }

    pub async fn next_lifecycle(&mut self) -> Option<GatewayLifecycleEventV1> {
        self.lifecycle
            .changed()
            .await
            .ok()
            .map(|()| *self.lifecycle.borrow_and_update())
    }

    pub fn current_lifecycle(&self) -> GatewayLifecycleEventV1 {
        *self.lifecycle.borrow()
    }
}

pub struct GatewayRuntimeControlV1 {
    commands: tokio::sync::watch::Receiver<GatewayCommandV1>,
    lifecycle: tokio::sync::watch::Sender<GatewayLifecycleEventV1>,
}

pub fn control_channel() -> (GatewayControlV1, GatewayRuntimeControlV1) {
    let (commands_tx, commands_rx) = tokio::sync::watch::channel(GatewayCommandV1::Serve);
    let (lifecycle_tx, lifecycle_rx) =
        tokio::sync::watch::channel(GatewayLifecycleEventV1::Starting);
    (
        GatewayControlV1 {
            commands: commands_tx,
            lifecycle: lifecycle_rx,
        },
        GatewayRuntimeControlV1 {
            commands: commands_rx,
            lifecycle: lifecycle_tx,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
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
) {
    let (control, runtime_control) = control_channel();
    let _keep_control_open = control;
    let _ = run_controlled(
        token,
        identity,
        ruleset,
        bindings,
        failure_message,
        instances,
        instance_ids,
        teardown,
        ruleset_store,
        snapshot_provider,
        runtime_control,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub async fn run_controlled(
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
    mut control: GatewayRuntimeControlV1,
) -> GatewayExitV1 {
    let http = Client::new(token.clone());
    let mutation = TwilightMutationAdapter::new(&http, identity.key.clone());
    let mut shard = Shard::new(ShardId::ONE, token, Intents::empty());
    let event_types = EventTypeFlags::INTERACTION_CREATE
        | EventTypeFlags::READY
        | EventTypeFlags::RESUMED
        | EventTypeFlags::GATEWAY_RECONNECT
        | EventTypeFlags::GATEWAY_INVALIDATE_SESSION;
    let mut commands_open = true;

    loop {
        tokio::select! {
            command = control.commands.changed(), if commands_open => {
                match command {
                    Ok(()) if *control.commands.borrow_and_update() == GatewayCommandV1::DrainAndShutdown => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Draining);
                        shard.close(CloseFrame::NORMAL);
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Drained);
                        return GatewayExitV1::Drained;
                    }
                    Ok(()) => {}
                    Err(_) => commands_open = false,
                }
            }
            item = shard.next_event(event_types) => {
                let Some(item) = item else {
                    control.lifecycle.send_replace(GatewayLifecycleEventV1::StreamEnded);
                    return GatewayExitV1::StreamEnded;
                };
                let event = match item {
                    Ok(event) => event,
                    Err(_) => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::ReceiveError);
                        continue;
                    }
                };
                match event {
                    Event::Ready(_) => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Serving {
                            kind: GatewayReadyKindV1::Ready,
                        });
                    }
                    Event::Resumed => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Serving {
                            kind: GatewayReadyKindV1::Resumed,
                        });
                    }
                    Event::GatewayClose(_) => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Disconnected {
                            kind: GatewayDisconnectKindV1::Close,
                        });
                    }
                    Event::GatewayReconnect => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Disconnected {
                            kind: GatewayDisconnectKindV1::Reconnect,
                        });
                    }
                    Event::GatewayInvalidateSession(_) => {
                        control.lifecycle.send_replace(GatewayLifecycleEventV1::Disconnected {
                            kind: GatewayDisconnectKindV1::SessionInvalidated,
                        });
                    }
                    Event::InteractionCreate(interaction_create) => {
                        handle_interaction(
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
                        )
                        .await;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{control_channel, GatewayCommandV1, GatewayLifecycleEventV1, GatewayReadyKindV1};

    #[tokio::test]
    async fn control_channel_delivers_drain_without_polling_lifecycle() {
        let (control, runtime) = control_channel();
        assert!(control.drain_and_shutdown());
        assert_eq!(
            *runtime.commands.borrow(),
            GatewayCommandV1::DrainAndShutdown
        );
    }

    #[tokio::test]
    async fn lifecycle_channel_preserves_ready_and_resumed_order() {
        let (mut control, runtime) = control_channel();
        runtime
            .lifecycle
            .send_replace(GatewayLifecycleEventV1::Serving {
                kind: GatewayReadyKindV1::Ready,
            });
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV1::Serving {
                kind: GatewayReadyKindV1::Ready,
            })
        );
        runtime
            .lifecycle
            .send_replace(GatewayLifecycleEventV1::Serving {
                kind: GatewayReadyKindV1::Resumed,
            });
        assert_eq!(
            control.next_lifecycle().await,
            Some(GatewayLifecycleEventV1::Serving {
                kind: GatewayReadyKindV1::Resumed,
            })
        );
    }
}
