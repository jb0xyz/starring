use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use automation_runtime::{
    shared_gateway_control_channel_with_policy_v3, GatewayAdmissionPolicyV3, GatewayControlConfigV3,
};
use automation_runtime_convergence::ProcessInstanceId;
use paused_discord_model::application::interaction::message_component::MessageComponentInteractionData;
use paused_discord_model::application::interaction::{
    Interaction, InteractionData, InteractionType,
};
use paused_discord_model::channel::message::component::ComponentType;
use paused_discord_model::id::marker::{
    ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker, UserMarker,
};
use paused_discord_model::id::Id;
use paused_discord_model::oauth::ApplicationIntegrationMap;
use paused_discord_model::user::User;
use tokio::sync::watch;

use super::test_support::driver;
use super::{
    RuntimeDiscordDispatchDrainActorPortV1, RuntimeDiscordDispatchDrainConfirmationV1,
    RuntimeDiscordDispatchDrainLaneV1, RuntimeDiscordDispatchDrainRequestV1,
    RuntimeDiscordGatewayExitV1,
};
use crate::discord_interaction_normalizer::ZeroizingPinnedDiscordInteractionV1;
use crate::gateway::compose_runtime_gateway_section_test_bootstrap_v2;

struct PendingSealLaneV1 {
    aborted: Arc<AtomicUsize>,
}

impl RuntimeDiscordDispatchDrainLaneV1 for PendingSealLaneV1 {
    fn has_in_flight_v1(&self) -> bool {
        false
    }

    fn reconcile_accepting_v1(&mut self) {}

    fn handle_raw_interaction_v1(&mut self, interaction: Box<ZeroizingPinnedDiscordInteractionV1>) {
        drop(interaction);
    }

    fn poll_next_completion_v1(
        &mut self,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    fn drain_until_v1(
        &mut self,
        _transition_sequence: u64,
        _deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async { true })
    }

    fn seal_until_v1(
        &mut self,
        _deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(std::future::pending())
    }

    fn abort_v1(&mut self) {
        self.aborted.fetch_add(1, Ordering::SeqCst);
    }
}

struct RecordingDispatchLaneV1 {
    handled: Arc<AtomicUsize>,
    completed: Arc<AtomicUsize>,
    in_flight: bool,
}

impl RuntimeDiscordDispatchDrainLaneV1 for RecordingDispatchLaneV1 {
    fn has_in_flight_v1(&self) -> bool {
        self.in_flight
    }

    fn reconcile_accepting_v1(&mut self) {}

    fn handle_raw_interaction_v1(&mut self, interaction: Box<ZeroizingPinnedDiscordInteractionV1>) {
        drop(interaction);
        self.handled.fetch_add(1, Ordering::SeqCst);
        self.in_flight = true;
    }

    fn poll_next_completion_v1(
        &mut self,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.in_flight = false;
            self.completed.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn drain_until_v1(
        &mut self,
        _transition_sequence: u64,
        _deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            if self.in_flight {
                self.in_flight = false;
                self.completed.fetch_add(1, Ordering::SeqCst);
            }
            true
        })
    }

    fn seal_until_v1(
        &mut self,
        _deadline: Instant,
    ) -> std::pin::Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            if self.in_flight {
                self.in_flight = false;
                self.completed.fetch_add(1, Ordering::SeqCst);
            }
            true
        })
    }

    fn abort_v1(&mut self) {
        self.in_flight = false;
    }
}

fn interaction_user_v1(id: u64) -> User {
    User {
        accent_color: None,
        avatar: None,
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 0,
        email: None,
        flags: None,
        global_name: None,
        id: Id::<UserMarker>::new(id),
        locale: None,
        mfa_enabled: None,
        name: String::new(),
        premium_type: None,
        primary_guild: None,
        public_flags: None,
        system: None,
        verified: None,
    }
}

#[allow(deprecated)]
fn button_interaction_v1() -> Interaction {
    Interaction {
        app_permissions: None,
        application_id: Id::<ApplicationMarker>::new(41),
        authorizing_integration_owners: ApplicationIntegrationMap {
            guild: None,
            user: None,
        },
        channel: None,
        channel_id: Some(Id::<ChannelMarker>::new(43)),
        context: None,
        data: Some(InteractionData::MessageComponent(Box::new(
            MessageComponentInteractionData {
                custom_id: "join".to_string(),
                component_type: ComponentType::Button,
                resolved: None,
                values: Vec::new(),
            },
        ))),
        entitlements: Vec::new(),
        guild: None,
        guild_id: Some(Id::<GuildMarker>::new(42)),
        guild_locale: None,
        id: Id::<InteractionMarker>::new(47),
        kind: InteractionType::MessageComponent,
        locale: Some("ko".to_string()),
        member: None,
        message: None,
        token: "interaction-token-secret".to_string(),
        user: Some(interaction_user_v1(53)),
    }
}

fn gateway() -> crate::RuntimeGatewayBootstrapV1 {
    compose_runtime_gateway_section_test_bootstrap_v2(
        ProcessInstanceId::parse("runtime-process:discord-serving-test").unwrap(),
    )
}

#[tokio::test]
async fn interaction_event_is_owned_dispatched_and_completed_by_actor_lane() {
    let mut gateway = gateway();
    let (events, driver, _polls, _closes, _drops) = driver();
    let handled = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let lane = RecordingDispatchLaneV1 {
        handled: handled.clone(),
        completed: completed.clone(),
        in_flight: false,
    };
    let shutdown_deadline = Instant::now() + Duration::from_secs(3);
    let supervisor = gateway
        .start_discord_gateway_with_driver_and_lane_v1(
            driver,
            Box::new(lane),
            Instant::now() + Duration::from_secs(2),
            shutdown_deadline,
        )
        .await
        .unwrap();
    events.send_interaction(button_interaction_v1()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if handled.load(Ordering::SeqCst) == 1 && completed.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let terminal = supervisor
        .shutdown_until(
            gateway.begin_discord_drain_until_v1(shutdown_deadline),
            shutdown_deadline,
        )
        .await
        .unwrap();
    assert_eq!(terminal.exit(), RuntimeDiscordGatewayExitV1::Commanded);
}

#[tokio::test]
async fn terminal_deadline_bounds_and_aborts_a_dispatch_seal_that_never_returns() {
    let (control, _runtime) = shared_gateway_control_channel_with_policy_v3(
        GatewayControlConfigV3::new(NonZeroUsize::MIN, NonZeroUsize::MIN).unwrap(),
        GatewayAdmissionPolicyV3::ExplicitResumeAfterEveryConnect,
    );
    let startup = control.current_admission_snapshot().transition_sequence();
    let (_requests_sender, requests) =
        watch::channel(RuntimeDiscordDispatchDrainRequestV1::startup_v1(startup));
    let (confirmations, _confirmation_observer) = watch::channel(
        RuntimeDiscordDispatchDrainConfirmationV1::startup_v1(startup),
    );
    let aborted = Arc::new(AtomicUsize::new(0));
    let mut port = RuntimeDiscordDispatchDrainActorPortV1::new(
        requests,
        confirmations,
        Box::new(PendingSealLaneV1 {
            aborted: aborted.clone(),
        }),
    )
    .unwrap();
    let outcome = tokio::time::timeout(
        Duration::from_millis(200),
        super::seal_runtime_discord_dispatch_lane_until_v1(
            &mut port,
            Instant::now() + Duration::from_millis(25),
        ),
    )
    .await
    .expect("actor-owned terminal dispatch deadline");
    assert!(!outcome);
    assert_eq!(aborted.load(Ordering::SeqCst), 1);
}
