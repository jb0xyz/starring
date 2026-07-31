use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use automation_core::InteractionResponder;
use automation_instance::{
    InstanceIdGenerator, InstanceRegistrarV1, InstanceRouteReaderV1, InstanceStoreError,
    InstanceTeardownRetryScanCursorV2, InstanceTeardownRetryScanPageV2,
    InstanceTeardownRetryScannerV2, InstanceTeardownStoreV1, SecureRandomInstanceIdGenerator,
};
use automation_instance_teardown::{
    InstanceTeardownService, Teardown, TeardownError, TeardownOutcome,
};
use automation_ruleset_dispatch::{GuildRoleSnapshotProvider, PinnedInstanceResolverV1};
use automation_runtime_registry::ServingSlotRegistryV1;
use discord_model::{ChannelId, GuildId, UserId};
use twilight_http::Client;
use twilight_model::application::interaction::message_component::MessageComponentInteractionData;
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData, ModalInteractionTextInput,
};
use twilight_model::application::interaction::{Interaction, InteractionData, InteractionType};
use twilight_model::channel::message::component::ComponentType;
use twilight_model::id::marker::{
    ApplicationMarker, ChannelMarker, GuildMarker, InteractionMarker, UserMarker,
};
use twilight_model::id::Id;
use twilight_model::oauth::ApplicationIntegrationMap;
use twilight_model::user::User;
use zeroize::{Zeroize, Zeroizing};

use crate::instance_deleter::OwnedTwilightInstanceDeleter;
use crate::responder::TwilightInteractionResponder;
use crate::runner::InteractionExecutionOutcomeV3;
use crate::shared_gateway_admission::{
    SharedGatewayAdmissionBudgetV3, SharedGatewayAdmissionConfigV3, SharedGatewayAdmissionErrorV3,
    SharedGatewayAdmissionReservationV3,
};
use crate::shared_gateway_control::{GatewayConnectionObserverV3, GatewayReadyLeaseV3};
use crate::shared_gateway_executor::execute_admitted_interaction_v3;
use crate::shared_gateway_router::{parse_shared_gateway_route_v1, SharedGatewayRouteErrorV1};
use crate::snapshot::OwnedTwilightGuildRoleSnapshotProvider;

pub const MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3: usize = 100;
pub const MAX_SHARED_GATEWAY_INTERACTION_TOKEN_BYTES_V3: usize = 4_096;
pub const MAX_SHARED_GATEWAY_INTERACTION_LOCALE_BYTES_V3: usize = 64;
pub const MAX_SHARED_GATEWAY_MODAL_INPUTS_V3: usize = 5;
pub const MAX_SHARED_GATEWAY_MODAL_INPUT_VALUE_BYTES_V3: usize = 4_000;
pub const MAX_SHARED_GATEWAY_MODAL_PAYLOAD_BYTES_V3: usize = 20_000;
const SHARED_GATEWAY_REJECTION_ACKNOWLEDGEMENT_TIMEOUT_V3: Duration = Duration::from_secs(2);
const SHARED_GATEWAY_MUTATION_HTTP_TIMEOUT_V3: Duration = Duration::from_secs(15);
pub const SHARED_GATEWAY_STABLE_FAILURE_MESSAGE_V3: &str =
    "Starring is temporarily unable to process this request. Please try again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SharedGatewayInteractionApplicationIdV3(NonZeroU64);

impl SharedGatewayInteractionApplicationIdV3 {
    pub fn new(value: u64) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(SharedGatewayInteractionEnvelopeErrorV3::Identity)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SharedGatewayInteractionIdV3(NonZeroU64);

impl SharedGatewayInteractionIdV3 {
    pub fn new(value: u64) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(SharedGatewayInteractionEnvelopeErrorV3::Identity)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SharedGatewayInteractionIdentityV3 {
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    application_id: SharedGatewayInteractionApplicationIdV3,
    interaction_id: SharedGatewayInteractionIdV3,
}

impl SharedGatewayInteractionIdentityV3 {
    pub fn new(
        guild_id: GuildId,
        channel_id: ChannelId,
        user_id: UserId,
        application_id: SharedGatewayInteractionApplicationIdV3,
        interaction_id: SharedGatewayInteractionIdV3,
    ) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        if guild_id.0 == 0 || channel_id.0 == 0 || user_id.0 == 0 {
            return Err(SharedGatewayInteractionEnvelopeErrorV3::Identity);
        }
        Ok(Self {
            guild_id,
            channel_id,
            user_id,
            application_id,
            interaction_id,
        })
    }

    pub fn guild_id(self) -> GuildId {
        self.guild_id
    }

    pub fn channel_id(self) -> ChannelId {
        self.channel_id
    }

    pub fn user_id(self) -> UserId {
        self.user_id
    }

    pub fn application_id(self) -> SharedGatewayInteractionApplicationIdV3 {
        self.application_id
    }

    pub fn interaction_id(self) -> SharedGatewayInteractionIdV3 {
        self.interaction_id
    }
}

pub struct SharedGatewayInteractionTokenV3(Zeroizing<String>);

impl SharedGatewayInteractionTokenV3 {
    pub fn new(value: String) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > MAX_SHARED_GATEWAY_INTERACTION_TOKEN_BYTES_V3 {
            return Err(SharedGatewayInteractionEnvelopeErrorV3::Token);
        }
        Ok(Self(value))
    }

    fn expose_v3(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for SharedGatewayInteractionTokenV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayInteractionTokenV3(<redacted>)")
    }
}

pub struct SharedGatewayModalInputV3 {
    component_id: i32,
    custom_id: String,
    value: Zeroizing<String>,
}

impl SharedGatewayModalInputV3 {
    pub fn new(
        component_id: i32,
        custom_id: String,
        value: String,
    ) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        let value = Zeroizing::new(value);
        validate_custom_id_v3(&custom_id)?;
        if value.len() > MAX_SHARED_GATEWAY_MODAL_INPUT_VALUE_BYTES_V3 {
            return Err(SharedGatewayInteractionEnvelopeErrorV3::ModalInput);
        }
        Ok(Self {
            component_id,
            custom_id,
            value,
        })
    }

    pub fn component_id(&self) -> i32 {
        self.component_id
    }

    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }
}

impl Debug for SharedGatewayModalInputV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayModalInputV3(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedGatewayInteractionKindV3 {
    MessageComponent,
    ModalSubmit,
}

enum SharedGatewayInteractionDataV3 {
    MessageComponent {
        custom_id: String,
    },
    ModalSubmit {
        custom_id: String,
        inputs: Vec<SharedGatewayModalInputV3>,
    },
}

pub struct SharedGatewayInteractionEnvelopeV3 {
    identity: SharedGatewayInteractionIdentityV3,
    locale: Option<String>,
    token: SharedGatewayInteractionTokenV3,
    data: SharedGatewayInteractionDataV3,
}

impl SharedGatewayInteractionEnvelopeV3 {
    pub fn message_component_v3(
        identity: SharedGatewayInteractionIdentityV3,
        custom_id: String,
        locale: Option<String>,
        token: SharedGatewayInteractionTokenV3,
    ) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        validate_custom_id_v3(&custom_id)?;
        validate_locale_v3(locale.as_deref())?;
        Ok(Self {
            identity,
            locale,
            token,
            data: SharedGatewayInteractionDataV3::MessageComponent { custom_id },
        })
    }

    pub fn modal_submit_v3(
        identity: SharedGatewayInteractionIdentityV3,
        custom_id: String,
        inputs: Vec<SharedGatewayModalInputV3>,
        locale: Option<String>,
        token: SharedGatewayInteractionTokenV3,
    ) -> Result<Self, SharedGatewayInteractionEnvelopeErrorV3> {
        validate_custom_id_v3(&custom_id)?;
        validate_locale_v3(locale.as_deref())?;
        if inputs.is_empty() || inputs.len() > MAX_SHARED_GATEWAY_MODAL_INPUTS_V3 {
            return Err(SharedGatewayInteractionEnvelopeErrorV3::ModalInputCount);
        }
        let mut custom_ids = BTreeSet::new();
        let mut payload_bytes = custom_id.len();
        for input in &inputs {
            if !custom_ids.insert(input.custom_id.as_str()) {
                return Err(SharedGatewayInteractionEnvelopeErrorV3::DuplicateModalInput);
            }
            payload_bytes = payload_bytes
                .checked_add(input.custom_id.len())
                .and_then(|total| total.checked_add(input.value.len()))
                .ok_or(SharedGatewayInteractionEnvelopeErrorV3::ModalPayload)?;
        }
        if payload_bytes > MAX_SHARED_GATEWAY_MODAL_PAYLOAD_BYTES_V3 {
            return Err(SharedGatewayInteractionEnvelopeErrorV3::ModalPayload);
        }
        Ok(Self {
            identity,
            locale,
            token,
            data: SharedGatewayInteractionDataV3::ModalSubmit { custom_id, inputs },
        })
    }

    pub fn identity_v3(&self) -> SharedGatewayInteractionIdentityV3 {
        self.identity
    }

    pub fn kind_v3(&self) -> SharedGatewayInteractionKindV3 {
        match self.data {
            SharedGatewayInteractionDataV3::MessageComponent { .. } => {
                SharedGatewayInteractionKindV3::MessageComponent
            }
            SharedGatewayInteractionDataV3::ModalSubmit { .. } => {
                SharedGatewayInteractionKindV3::ModalSubmit
            }
        }
    }

    pub fn custom_id_v3(&self) -> &str {
        match &self.data {
            SharedGatewayInteractionDataV3::MessageComponent { custom_id }
            | SharedGatewayInteractionDataV3::ModalSubmit { custom_id, .. } => custom_id,
        }
    }

    pub fn locale_v3(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    pub(crate) fn twilight_interaction_v3(&self) -> ZeroizingTwilightInteractionV3 {
        let data = match &self.data {
            SharedGatewayInteractionDataV3::MessageComponent { custom_id } => {
                InteractionData::MessageComponent(Box::new(MessageComponentInteractionData {
                    custom_id: custom_id.clone(),
                    component_type: ComponentType::Button,
                    resolved: None,
                    values: Vec::new(),
                }))
            }
            SharedGatewayInteractionDataV3::ModalSubmit { custom_id, inputs } => {
                InteractionData::ModalSubmit(Box::new(ModalInteractionData {
                    components: inputs
                        .iter()
                        .map(|input| {
                            ModalInteractionComponent::TextInput(ModalInteractionTextInput {
                                custom_id: input.custom_id.clone(),
                                id: input.component_id,
                                value: input.value.to_string(),
                            })
                        })
                        .collect(),
                    custom_id: custom_id.clone(),
                    resolved: None,
                }))
            }
        };
        let identity = self.identity;
        #[allow(deprecated)]
        let interaction = Interaction {
            app_permissions: None,
            application_id: Id::<ApplicationMarker>::new(identity.application_id.get()),
            authorizing_integration_owners: ApplicationIntegrationMap {
                guild: None,
                user: None,
            },
            channel: None,
            channel_id: Some(Id::<ChannelMarker>::new(identity.channel_id.0)),
            context: None,
            data: Some(data),
            entitlements: Vec::new(),
            guild: None,
            guild_id: Some(Id::<GuildMarker>::new(identity.guild_id.0)),
            guild_locale: None,
            id: Id::<InteractionMarker>::new(identity.interaction_id.get()),
            kind: match self.kind_v3() {
                SharedGatewayInteractionKindV3::MessageComponent => {
                    InteractionType::MessageComponent
                }
                SharedGatewayInteractionKindV3::ModalSubmit => InteractionType::ModalSubmit,
            },
            locale: self.locale.clone(),
            member: None,
            message: None,
            token: self.token.expose_v3().to_string(),
            user: Some(minimal_user_v3(identity.user_id)),
        };
        ZeroizingTwilightInteractionV3(interaction)
    }

    pub(crate) fn from_twilight_interaction_v3(
        interaction: Interaction,
    ) -> Result<Option<Self>, SharedGatewayInteractionEnvelopeErrorV3> {
        let interaction = ZeroizingTwilightInteractionV3(interaction);
        let Some(guild_id) = interaction.0.guild_id else {
            return Ok(None);
        };
        let Some(channel_id) = twilight_channel_id_v3(&interaction.0) else {
            return Ok(None);
        };
        let Some(user_id) = interaction.0.author_id() else {
            return Ok(None);
        };
        let identity = SharedGatewayInteractionIdentityV3::new(
            GuildId(guild_id.get()),
            ChannelId(channel_id.get()),
            UserId(user_id.get()),
            SharedGatewayInteractionApplicationIdV3::new(interaction.0.application_id.get())?,
            SharedGatewayInteractionIdV3::new(interaction.0.id.get())?,
        )?;
        let token = SharedGatewayInteractionTokenV3::new(interaction.0.token.clone())?;
        match interaction.0.data.as_ref() {
            Some(InteractionData::MessageComponent(data)) => Self::message_component_v3(
                identity,
                data.custom_id.clone(),
                interaction.0.locale.clone(),
                token,
            )
            .map(Some),
            Some(InteractionData::ModalSubmit(data)) => {
                let mut inputs = Vec::new();
                collect_twilight_modal_inputs_v3(&data.components, &mut inputs)?;
                Self::modal_submit_v3(
                    identity,
                    data.custom_id.clone(),
                    inputs,
                    interaction.0.locale.clone(),
                    token,
                )
                .map(Some)
            }
            _ => Ok(None),
        }
    }
}

impl Debug for SharedGatewayInteractionEnvelopeV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayInteractionEnvelopeV3(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SharedGatewayInteractionEnvelopeErrorV3 {
    #[error("shared gateway interaction identity is invalid")]
    Identity,
    #[error("shared gateway interaction custom id is invalid")]
    CustomId,
    #[error("shared gateway interaction locale is invalid")]
    Locale,
    #[error("shared gateway interaction token is invalid")]
    Token,
    #[error("shared gateway modal input count is invalid")]
    ModalInputCount,
    #[error("shared gateway modal input is invalid")]
    ModalInput,
    #[error("shared gateway modal input ids are not unique")]
    DuplicateModalInput,
    #[error("shared gateway modal payload is too large")]
    ModalPayload,
}

impl SharedGatewayInteractionEnvelopeErrorV3 {
    pub fn code(self) -> &'static str {
        match self {
            Self::Identity => "shared_gateway_interaction_identity_invalid",
            Self::CustomId => "shared_gateway_interaction_custom_id_invalid",
            Self::Locale => "shared_gateway_interaction_locale_invalid",
            Self::Token => "shared_gateway_interaction_token_invalid",
            Self::ModalInputCount => "shared_gateway_modal_input_count_invalid",
            Self::ModalInput => "shared_gateway_modal_input_invalid",
            Self::DuplicateModalInput => "shared_gateway_modal_input_duplicate",
            Self::ModalPayload => "shared_gateway_modal_payload_too_large",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedGatewayInteractionRejectionV3 {
    Route(SharedGatewayRouteErrorV1),
    Admission(SharedGatewayAdmissionErrorV3),
}

impl SharedGatewayInteractionRejectionV3 {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Route(error) => error.code(),
            Self::Admission(error) => error.code(),
        }
    }
}

pub enum SharedGatewayInteractionReservationOutcomeV3 {
    Reserved(Box<SharedGatewayReservedInteractionV3>),
    Ignored,
    Rejected {
        reason: SharedGatewayInteractionRejectionV3,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    },
}

impl Debug for SharedGatewayInteractionReservationOutcomeV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayInteractionReservationOutcomeV3(<redacted>)")
    }
}

pub struct SharedGatewayReservedInteractionV3 {
    reservation: SharedGatewayAdmissionReservationV3,
    envelope: SharedGatewayInteractionEnvelopeV3,
}

impl Debug for SharedGatewayReservedInteractionV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayReservedInteractionV3(<redacted>)")
    }
}

pub enum SharedGatewayInteractionDispatchOutcomeV3 {
    Executed(InteractionExecutionOutcomeV3),
    Ignored,
    Rejected {
        error: SharedGatewayAdmissionErrorV3,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    },
}

impl Debug for SharedGatewayInteractionDispatchOutcomeV3 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SharedGatewayInteractionDispatchOutcomeV3(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedGatewayRejectionAcknowledgementOutcomeV3 {
    Sent,
    Failed,
    TimedOut,
}

pub async fn acknowledge_shared_gateway_interaction_rejection_v3(
    interaction_http: &Client,
    envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    failure_message: &str,
) -> SharedGatewayRejectionAcknowledgementOutcomeV3 {
    let interaction = envelope.twilight_interaction_v3();
    let responder = TwilightInteractionResponder::from_interaction(
        interaction_http,
        interaction.as_interaction_v3(),
        "",
    );
    match tokio::time::timeout(
        SHARED_GATEWAY_REJECTION_ACKNOWLEDGEMENT_TIMEOUT_V3,
        responder.respond_ephemeral(failure_message.to_string()),
    )
    .await
    {
        Ok(Ok(())) => SharedGatewayRejectionAcknowledgementOutcomeV3::Sent,
        Ok(Err(_)) => SharedGatewayRejectionAcknowledgementOutcomeV3::Failed,
        Err(_) => SharedGatewayRejectionAcknowledgementOutcomeV3::TimedOut,
    }
}

pub fn reserve_shared_gateway_interaction_v3(
    envelope: SharedGatewayInteractionEnvelopeV3,
    ready_lease: Option<GatewayReadyLeaseV3>,
    observer: &GatewayConnectionObserverV3,
    admission_budget: &SharedGatewayAdmissionBudgetV3,
) -> SharedGatewayInteractionReservationOutcomeV3 {
    match parse_shared_gateway_route_v1(envelope.identity.guild_id, envelope.custom_id_v3()) {
        Ok(Some(_)) => {}
        Ok(None) => return SharedGatewayInteractionReservationOutcomeV3::Ignored,
        Err(error) => {
            return SharedGatewayInteractionReservationOutcomeV3::Rejected {
                reason: SharedGatewayInteractionRejectionV3::Route(error),
                envelope: Box::new(envelope),
            }
        }
    }
    let Some(ready_lease) = ready_lease else {
        return SharedGatewayInteractionReservationOutcomeV3::Rejected {
            reason: SharedGatewayInteractionRejectionV3::Admission(
                SharedGatewayAdmissionErrorV3::NotReady,
            ),
            envelope: Box::new(envelope),
        };
    };
    match admission_budget.try_reserve(observer, &ready_lease) {
        Ok(reservation) => SharedGatewayInteractionReservationOutcomeV3::Reserved(Box::new(
            SharedGatewayReservedInteractionV3 {
                reservation,
                envelope,
            },
        )),
        Err(error) => SharedGatewayInteractionReservationOutcomeV3::Rejected {
            reason: SharedGatewayInteractionRejectionV3::Admission(error),
            envelope: Box::new(envelope),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_reserved_shared_gateway_interaction_v3<I, G, T, PR, S>(
    reserved: SharedGatewayReservedInteractionV3,
    registry: &ServingSlotRegistryV1,
    instances: &I,
    instance_ids: &G,
    teardown: &T,
    pinned_resolver: &PR,
    snapshot_provider: &S,
    mutation_http: &Client,
    interaction_http: &Client,
    failure_message: &str,
) -> SharedGatewayInteractionDispatchOutcomeV3
where
    I: InstanceRouteReaderV1 + InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
    PR: PinnedInstanceResolverV1,
    S: GuildRoleSnapshotProvider,
{
    let SharedGatewayReservedInteractionV3 {
        reservation,
        envelope,
    } = reserved;
    match reservation
        .admit(
            registry,
            instances,
            envelope.identity.guild_id,
            envelope.custom_id_v3(),
        )
        .await
    {
        Ok(Some(admitted)) => {
            let interaction = envelope.twilight_interaction_v3();
            SharedGatewayInteractionDispatchOutcomeV3::Executed(
                execute_admitted_interaction_v3(
                    mutation_http,
                    interaction_http,
                    admitted,
                    interaction.as_interaction_v3(),
                    failure_message,
                    instances,
                    instance_ids,
                    teardown,
                    pinned_resolver,
                    snapshot_provider,
                )
                .await,
            )
        }
        Ok(None) => SharedGatewayInteractionDispatchOutcomeV3::Ignored,
        Err(error) => SharedGatewayInteractionDispatchOutcomeV3::Rejected {
            error,
            envelope: Box::new(envelope),
        },
    }
}

pub fn cancel_reserved_shared_gateway_interaction_v3(
    reserved: SharedGatewayReservedInteractionV3,
) -> Box<SharedGatewayInteractionEnvelopeV3> {
    let SharedGatewayReservedInteractionV3 {
        reservation,
        envelope,
    } = reserved;
    drop(reservation);
    Box::new(envelope)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OwnedSharedGatewayDispatchServicesCompositionErrorV3 {
    #[error("shared gateway dispatch service composition timed out")]
    TimedOut,
    #[error("shared gateway dispatch role snapshot provider is unavailable")]
    SnapshotUnavailable,
}

impl OwnedSharedGatewayDispatchServicesCompositionErrorV3 {
    pub fn code(self) -> &'static str {
        match self {
            Self::TimedOut => "shared_gateway_dispatch_composition_timed_out",
            Self::SnapshotUnavailable => "shared_gateway_dispatch_snapshot_unavailable",
        }
    }
}

pub struct OwnedSharedGatewayDispatchServicesV3<I> {
    registry: ServingSlotRegistryV1,
    instances: I,
    instance_ids: SecureRandomInstanceIdGenerator,
    teardown: Arc<Teardown<I, OwnedTwilightInstanceDeleter>>,
    snapshot_provider: OwnedTwilightGuildRoleSnapshotProvider,
    mutation_http: Arc<Client>,
    interaction_http: Arc<Client>,
    admission_budget: SharedGatewayAdmissionBudgetV3,
}

impl<I> OwnedSharedGatewayDispatchServicesV3<I>
where
    I: Clone
        + Send
        + Sync
        + 'static
        + InstanceRouteReaderV1
        + InstanceRegistrarV1
        + InstanceTeardownStoreV1
        + PinnedInstanceResolverV1,
{
    pub async fn compose_v3(
        token: Zeroizing<String>,
        registry: ServingSlotRegistryV1,
        instances: I,
        admission_config: SharedGatewayAdmissionConfigV3,
        operation_deadline: Instant,
    ) -> Result<Self, OwnedSharedGatewayDispatchServicesCompositionErrorV3> {
        if Instant::now() >= operation_deadline {
            return Err(OwnedSharedGatewayDispatchServicesCompositionErrorV3::TimedOut);
        }
        let mutation_http = Arc::new(
            Client::builder()
                .token(token.to_string())
                .timeout(SHARED_GATEWAY_MUTATION_HTTP_TIMEOUT_V3)
                .build(),
        );
        let interaction_http = Arc::new(
            Client::builder()
                .token(token.to_string())
                .ratelimiter(None)
                .timeout(SHARED_GATEWAY_REJECTION_ACKNOWLEDGEMENT_TIMEOUT_V3)
                .build(),
        );
        let snapshot_provider = tokio::time::timeout_at(
            tokio::time::Instant::from_std(operation_deadline),
            OwnedTwilightGuildRoleSnapshotProvider::new(Arc::clone(&mutation_http)),
        )
        .await
        .map_err(|_| OwnedSharedGatewayDispatchServicesCompositionErrorV3::TimedOut)?
        .map_err(|_| OwnedSharedGatewayDispatchServicesCompositionErrorV3::SnapshotUnavailable)?;
        let teardown = Arc::new(Teardown::new(
            instances.clone(),
            OwnedTwilightInstanceDeleter::new(Arc::clone(&mutation_http)),
        ));
        Ok(Self {
            registry,
            instances,
            instance_ids: SecureRandomInstanceIdGenerator::new(),
            teardown,
            snapshot_provider,
            mutation_http,
            interaction_http,
            admission_budget: SharedGatewayAdmissionBudgetV3::new(admission_config),
        })
    }

    pub fn dispatch_capacity_v3(&self) -> std::num::NonZeroUsize {
        self.admission_budget.capacity()
    }

    pub fn reserve_v3(
        &self,
        envelope: SharedGatewayInteractionEnvelopeV3,
        ready_lease: Option<GatewayReadyLeaseV3>,
        observer: &GatewayConnectionObserverV3,
    ) -> SharedGatewayInteractionReservationOutcomeV3 {
        reserve_shared_gateway_interaction_v3(
            envelope,
            ready_lease,
            observer,
            &self.admission_budget,
        )
    }

    pub fn cancel_v3(
        &self,
        reserved: SharedGatewayReservedInteractionV3,
    ) -> Box<SharedGatewayInteractionEnvelopeV3> {
        cancel_reserved_shared_gateway_interaction_v3(reserved)
    }

    pub async fn dispatch_v3(
        &self,
        reserved: SharedGatewayReservedInteractionV3,
    ) -> SharedGatewayInteractionDispatchOutcomeV3 {
        dispatch_reserved_shared_gateway_interaction_v3(
            reserved,
            &self.registry,
            &self.instances,
            &self.instance_ids,
            self.teardown.as_ref(),
            &self.instances,
            &self.snapshot_provider,
            &self.mutation_http,
            &self.interaction_http,
            SHARED_GATEWAY_STABLE_FAILURE_MESSAGE_V3,
        )
        .await
    }

    pub async fn acknowledge_rejection_v3(
        &self,
        envelope: Box<SharedGatewayInteractionEnvelopeV3>,
    ) -> SharedGatewayRejectionAcknowledgementOutcomeV3 {
        acknowledge_shared_gateway_interaction_rejection_v3(
            &self.interaction_http,
            envelope,
            SHARED_GATEWAY_STABLE_FAILURE_MESSAGE_V3,
        )
        .await
    }
}

impl<I> OwnedSharedGatewayDispatchServicesV3<I>
where
    I: Clone
        + Send
        + Sync
        + 'static
        + InstanceRouteReaderV1
        + InstanceRegistrarV1
        + InstanceTeardownRetryScannerV2
        + InstanceTeardownStoreV1
        + PinnedInstanceResolverV1,
{
    pub async fn scan_teardown_retries_v1(
        &self,
        cursor: &InstanceTeardownRetryScanCursorV2,
        limit: NonZeroUsize,
    ) -> Result<InstanceTeardownRetryScanPageV2, InstanceStoreError> {
        self.instances.scan_retryable_v2(cursor, limit).await
    }

    pub async fn retry_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: automation_instance::InstanceId,
    ) -> Result<TeardownOutcome, TeardownError> {
        self.teardown.teardown(guild_id, instance_id).await
    }
}

impl<I> Debug for OwnedSharedGatewayDispatchServicesV3<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnedSharedGatewayDispatchServicesV3(<redacted>)")
    }
}

fn validate_custom_id_v3(custom_id: &str) -> Result<(), SharedGatewayInteractionEnvelopeErrorV3> {
    if custom_id.is_empty() || custom_id.len() > MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3 {
        return Err(SharedGatewayInteractionEnvelopeErrorV3::CustomId);
    }
    Ok(())
}

fn validate_locale_v3(locale: Option<&str>) -> Result<(), SharedGatewayInteractionEnvelopeErrorV3> {
    if locale.is_some_and(|value| {
        value.is_empty()
            || value.len() > MAX_SHARED_GATEWAY_INTERACTION_LOCALE_BYTES_V3
            || value.chars().any(char::is_control)
    }) {
        return Err(SharedGatewayInteractionEnvelopeErrorV3::Locale);
    }
    Ok(())
}

#[allow(deprecated)]
fn twilight_channel_id_v3(interaction: &Interaction) -> Option<Id<ChannelMarker>> {
    interaction
        .channel
        .as_ref()
        .map(|channel| channel.id)
        .or(interaction.channel_id)
}

fn collect_twilight_modal_inputs_v3(
    components: &[ModalInteractionComponent],
    inputs: &mut Vec<SharedGatewayModalInputV3>,
) -> Result<(), SharedGatewayInteractionEnvelopeErrorV3> {
    for component in components {
        match component {
            ModalInteractionComponent::TextInput(input) => {
                inputs.push(SharedGatewayModalInputV3::new(
                    input.id,
                    input.custom_id.clone(),
                    input.value.clone(),
                )?);
            }
            ModalInteractionComponent::ActionRow(row) => {
                collect_twilight_modal_inputs_v3(&row.components, inputs)?;
            }
            ModalInteractionComponent::Label(label) => {
                collect_twilight_modal_inputs_v3(
                    std::slice::from_ref(label.component.as_ref()),
                    inputs,
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn minimal_user_v3(user_id: UserId) -> User {
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
        id: Id::<UserMarker>::new(user_id.0),
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

pub(crate) struct ZeroizingTwilightInteractionV3(Interaction);

impl ZeroizingTwilightInteractionV3 {
    pub(crate) fn as_interaction_v3(&self) -> &Interaction {
        &self.0
    }
}

impl Drop for ZeroizingTwilightInteractionV3 {
    fn drop(&mut self) {
        zeroize_twilight_interaction_v3(&mut self.0);
    }
}

fn zeroize_twilight_interaction_v3(interaction: &mut Interaction) {
    interaction.token.zeroize();
    if let Some(InteractionData::ModalSubmit(data)) = interaction.data.as_mut() {
        zeroize_twilight_modal_inputs_v3(&mut data.components);
    }
}

fn zeroize_twilight_modal_inputs_v3(components: &mut [ModalInteractionComponent]) {
    for component in components {
        match component {
            ModalInteractionComponent::TextInput(input) => input.value.zeroize(),
            ModalInteractionComponent::ActionRow(row) => {
                zeroize_twilight_modal_inputs_v3(&mut row.components);
            }
            ModalInteractionComponent::Label(label) => {
                zeroize_twilight_modal_inputs_v3(std::slice::from_mut(label.component.as_mut()));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use crate::custom_id::encode_button;
    use crate::interaction_to_event;
    use crate::shared_gateway_admission::SharedGatewayAdmissionConfigV3;
    use crate::shared_gateway_control::{
        shared_gateway_control_channel_v3, GatewayControlConfigV3, GatewayReadyKindV3,
    };

    use super::*;
    use static_assertions::assert_not_impl_any;

    assert_not_impl_any!(SharedGatewayInteractionEnvelopeV3: Clone, serde::Serialize);
    assert_not_impl_any!(SharedGatewayInteractionTokenV3: Clone, serde::Serialize);
    assert_not_impl_any!(SharedGatewayModalInputV3: Clone, serde::Serialize);

    fn identity() -> SharedGatewayInteractionIdentityV3 {
        SharedGatewayInteractionIdentityV3::new(
            GuildId(7),
            ChannelId(8),
            UserId(9),
            SharedGatewayInteractionApplicationIdV3::new(10).unwrap(),
            SharedGatewayInteractionIdV3::new(11).unwrap(),
        )
        .unwrap()
    }

    fn token() -> SharedGatewayInteractionTokenV3 {
        SharedGatewayInteractionTokenV3::new("interaction-secret".to_string()).unwrap()
    }

    fn button() -> SharedGatewayInteractionEnvelopeV3 {
        SharedGatewayInteractionEnvelopeV3::message_component_v3(
            identity(),
            encode_button(GuildId(7), "study", "create"),
            Some("ko".to_string()),
            token(),
        )
        .unwrap()
    }

    #[test]
    fn source_neutral_envelope_preserves_exact_ids_and_redacts_secrets() {
        fn assert_send<T: Send>() {}
        let token = token();
        assert!(!format!("{token:?}").contains("interaction-secret"));
        let input =
            SharedGatewayModalInputV3::new(17, "room_name".to_string(), "secret-room".to_string())
                .unwrap();
        assert!(!format!("{input:?}").contains("secret-room"));
        let envelope = SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
            identity(),
            "starring:7:study:modal:room".to_string(),
            vec![input],
            Some("ko".to_string()),
            token,
        )
        .unwrap();
        assert_send::<SharedGatewayInteractionEnvelopeV3>();
        assert_eq!(envelope.identity_v3(), identity());
        assert_eq!(
            envelope.kind_v3(),
            SharedGatewayInteractionKindV3::ModalSubmit
        );
        assert_eq!(envelope.locale_v3(), Some("ko"));
        let debug = format!("{envelope:?}");
        assert!(!debug.contains("interaction-secret"));
        assert!(!debug.contains("secret-room"));

        let interaction = envelope.twilight_interaction_v3();
        assert_eq!(interaction.0.guild_id.unwrap().get(), 7);
        #[allow(deprecated)]
        let channel_id = interaction.0.channel_id.unwrap().get();
        assert_eq!(channel_id, 8);
        assert_eq!(interaction.0.author_id().unwrap().get(), 9);
        assert_eq!(interaction.0.application_id.get(), 10);
        assert_eq!(interaction.0.id.get(), 11);
        let Some(InteractionData::ModalSubmit(modal)) = interaction.0.data.as_ref() else {
            panic!("modal envelope must reconstruct modal interaction data")
        };
        assert_eq!(modal.custom_id, "starring:7:study:modal:room");
        let ModalInteractionComponent::TextInput(input) = &modal.components[0] else {
            panic!("modal input must retain its exact type")
        };
        assert_eq!(input.id, 17);
        assert_eq!(input.custom_id, "room_name");
        assert_eq!(input.value, "secret-room");
    }

    #[test]
    fn legacy_and_source_neutral_paths_produce_identical_dispatch_inputs() {
        let canonical = SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
            identity(),
            "starring:7:study:modal:room".to_string(),
            vec![SharedGatewayModalInputV3::new(
                17,
                "room_name".to_string(),
                "secret-room".to_string(),
            )
            .unwrap()],
            Some("ko".to_string()),
            token(),
        )
        .unwrap();
        let canonical_view = canonical.twilight_interaction_v3();
        let normalized = SharedGatewayInteractionEnvelopeV3::from_twilight_interaction_v3(
            canonical_view.0.clone(),
        )
        .unwrap()
        .unwrap();
        let normalized_view = normalized.twilight_interaction_v3();

        assert_eq!(
            interaction_to_event(canonical_view.as_interaction_v3(), "study"),
            interaction_to_event(normalized_view.as_interaction_v3(), "study")
        );
        assert_eq!(
            parse_shared_gateway_route_v1(
                canonical.identity_v3().guild_id(),
                canonical.custom_id_v3(),
            ),
            parse_shared_gateway_route_v1(
                normalized.identity_v3().guild_id(),
                normalized.custom_id_v3(),
            )
        );
        assert_eq!(canonical.identity_v3(), normalized.identity_v3());
        assert_eq!(canonical.kind_v3(), normalized.kind_v3());
        assert_eq!(canonical.locale_v3(), normalized.locale_v3());
        assert_eq!(
            (
                canonical_view.0.application_id,
                canonical_view.0.id,
                canonical_view.0.guild_id,
                canonical_view.0.author_id(),
                canonical_view.0.token.as_str(),
            ),
            (
                normalized_view.0.application_id,
                normalized_view.0.id,
                normalized_view.0.guild_id,
                normalized_view.0.author_id(),
                normalized_view.0.token.as_str(),
            )
        );
        #[allow(deprecated)]
        let channels = (canonical_view.0.channel_id, normalized_view.0.channel_id);
        assert_eq!(channels.0, channels.1);
    }

    #[test]
    fn envelope_rejects_unbounded_and_ambiguous_payloads() {
        assert_eq!(
            SharedGatewayInteractionTokenV3::new(String::new()).unwrap_err(),
            SharedGatewayInteractionEnvelopeErrorV3::Token
        );
        assert_eq!(
            SharedGatewayInteractionTokenV3::new(
                "x".repeat(MAX_SHARED_GATEWAY_INTERACTION_TOKEN_BYTES_V3 + 1)
            )
            .unwrap_err(),
            SharedGatewayInteractionEnvelopeErrorV3::Token
        );
        assert_eq!(
            SharedGatewayInteractionEnvelopeV3::message_component_v3(
                identity(),
                "x".repeat(MAX_SHARED_GATEWAY_CUSTOM_ID_BYTES_V3 + 1),
                None,
                token(),
            )
            .unwrap_err(),
            SharedGatewayInteractionEnvelopeErrorV3::CustomId
        );
        let duplicate = vec![
            SharedGatewayModalInputV3::new(1, "name".to_string(), "a".to_string()).unwrap(),
            SharedGatewayModalInputV3::new(2, "name".to_string(), "b".to_string()).unwrap(),
        ];
        assert_eq!(
            SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
                identity(),
                "starring:7:study:modal:room".to_string(),
                duplicate,
                None,
                token(),
            )
            .unwrap_err(),
            SharedGatewayInteractionEnvelopeErrorV3::DuplicateModalInput
        );
        let aggregate = (0..MAX_SHARED_GATEWAY_MODAL_INPUTS_V3)
            .map(|index| {
                SharedGatewayModalInputV3::new(
                    index as i32,
                    format!("field_{index}"),
                    "x".repeat(MAX_SHARED_GATEWAY_MODAL_INPUT_VALUE_BYTES_V3),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(
            SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
                identity(),
                "starring:7:study:modal:room".to_string(),
                aggregate,
                None,
                token(),
            )
            .unwrap_err(),
            SharedGatewayInteractionEnvelopeErrorV3::ModalPayload
        );
    }

    #[test]
    fn private_twilight_view_zeroizes_token_and_modal_values() {
        let envelope = SharedGatewayInteractionEnvelopeV3::modal_submit_v3(
            identity(),
            "starring:7:study:modal:room".to_string(),
            vec![SharedGatewayModalInputV3::new(
                17,
                "room_name".to_string(),
                "secret-room".to_string(),
            )
            .unwrap()],
            None,
            token(),
        )
        .unwrap();
        let mut interaction = envelope.twilight_interaction_v3();
        zeroize_twilight_interaction_v3(&mut interaction.0);
        assert!(interaction.0.token.is_empty());
        let Some(InteractionData::ModalSubmit(modal)) = interaction.0.data.as_ref() else {
            panic!("modal envelope must reconstruct modal interaction data")
        };
        let ModalInteractionComponent::TextInput(input) = &modal.components[0] else {
            panic!("modal input must retain its exact type")
        };
        assert!(input.value.is_empty());
    }

    #[test]
    fn reservation_is_source_independent_bounded_and_fail_closed() {
        let budget = SharedGatewayAdmissionBudgetV3::new(
            SharedGatewayAdmissionConfigV3::new(NonZeroUsize::new(1).unwrap()).unwrap(),
        );
        let (control, mut runtime) =
            shared_gateway_control_channel_v3(GatewayControlConfigV3::default());
        let observer = control.connection_observer();

        assert!(matches!(
            reserve_shared_gateway_interaction_v3(button(), None, &observer, &budget),
            SharedGatewayInteractionReservationOutcomeV3::Rejected {
                reason: SharedGatewayInteractionRejectionV3::Admission(
                    SharedGatewayAdmissionErrorV3::NotReady
                ),
                ..
            }
        ));

        let epoch = runtime.mark_connected(GatewayReadyKindV3::Ready).unwrap();
        let lease = observer.issue_ready_lease(epoch).unwrap();
        let reserved =
            reserve_shared_gateway_interaction_v3(button(), Some(lease), &observer, &budget);
        assert!(matches!(
            &reserved,
            SharedGatewayInteractionReservationOutcomeV3::Reserved(_)
        ));
        assert!(matches!(
            reserve_shared_gateway_interaction_v3(button(), Some(lease), &observer, &budget),
            SharedGatewayInteractionReservationOutcomeV3::Rejected {
                reason: SharedGatewayInteractionRejectionV3::Admission(
                    SharedGatewayAdmissionErrorV3::Overloaded
                ),
                ..
            }
        ));
        let SharedGatewayInteractionReservationOutcomeV3::Reserved(reserved) = reserved else {
            panic!("interaction must remain reserved")
        };
        let cancelled = cancel_reserved_shared_gateway_interaction_v3(*reserved);
        assert_eq!(
            cancelled.custom_id_v3(),
            encode_button(GuildId(7), "study", "create")
        );
        assert!(matches!(
            reserve_shared_gateway_interaction_v3(button(), Some(lease), &observer, &budget),
            SharedGatewayInteractionReservationOutcomeV3::Reserved(_)
        ));
    }
}
