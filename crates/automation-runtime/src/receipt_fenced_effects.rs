use std::fmt::{Debug, Formatter};

use automation_core::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    InteractionResponder, ModalPresentation, PostPanelSpec,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceRegistrarV1, InstanceStoreError,
};
use automation_instance_teardown::{InstanceTeardownService, TeardownError, TeardownOutcome};
use automation_state::{ModalFieldSpec, ModalFieldStyle, ModalInputPolicy};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use sha2::{Digest, Sha256};

const CANONICAL_VERSION_V1: u16 = 1;
const INITIAL_RESPONSE_OPERATION_DOMAIN_V1: &[u8] =
    b"starring.runtime.receipt_fenced.initial_response.operation.v1\0";
const INITIAL_RESPONSE_INTENT_DOMAIN_V1: &[u8] =
    b"starring.runtime.receipt_fenced.initial_response.intent.v1\0";
const INITIAL_RESPONSE_RESULT_DOMAIN_V1: &[u8] =
    b"starring.runtime.receipt_fenced.initial_response.result.v1\0";
const MODAL_DOMAIN_V1: &[u8] = b"starring.runtime.receipt_fenced.modal.v1\0";
const MODAL_FIELD_DOMAIN_V1: &[u8] = b"starring.runtime.receipt_fenced.modal_field.v1\0";
const RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1: &str =
    "interaction effect receipt persistence failed";
const INITIAL_RESPONSE_FAILURE_MESSAGE_V1: &str = "Discord initial response failed";
const EXECUTION_FAILURE_MESSAGE_V1: &str = "Discord execution failed";
const TEARDOWN_RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1: &str =
    "interaction teardown receipt persistence failed";
const INSTANCE_REGISTRATION_RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1: &str =
    "interaction instance registration receipt persistence failed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionInitialResponseKindV1 {
    RespondEphemeral,
    OpenModal,
    DeferEphemeral,
}

impl InteractionInitialResponseKindV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::RespondEphemeral => "respond_ephemeral",
            Self::OpenModal => "open_modal",
            Self::DeferEphemeral => "defer_ephemeral",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionInitialResponseResultKindV1 {
    Succeeded,
    DefinitiveFailure,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionInitialResponseIntentDispositionV1 {
    ExternalCallAuthorized,
    ExactReplaySuppressed,
}

impl InteractionInitialResponseIntentDispositionV1 {
    pub const fn external_call_authorized(self) -> bool {
        matches!(self, Self::ExternalCallAuthorized)
    }
}

impl InteractionInitialResponseResultKindV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::DefinitiveFailure => "definitive_failure",
            Self::Indeterminate => "indeterminate",
        }
    }
}

macro_rules! define_digest_v1 {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            pub fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            fn from_canonical_bytes(bytes: &[u8]) -> Self {
                Self(Sha256::digest(bytes).into())
            }
        }

        impl Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

define_digest_v1!(InteractionInitialResponseIntentDigestV1);
define_digest_v1!(InteractionInitialResponseResultDigestV1);

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionInitialResponseIntentV1 {
    kind: InteractionInitialResponseKindV1,
    digest: InteractionInitialResponseIntentDigestV1,
}

impl InteractionInitialResponseIntentV1 {
    pub const fn kind(&self) -> InteractionInitialResponseKindV1 {
        self.kind
    }

    pub fn digest(&self) -> &InteractionInitialResponseIntentDigestV1 {
        &self.digest
    }
}

impl Debug for InteractionInitialResponseIntentV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionInitialResponseIntentV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionInitialResponseResultV1 {
    intent_digest: InteractionInitialResponseIntentDigestV1,
    result: InteractionInitialResponseResultKindV1,
    digest: InteractionInitialResponseResultDigestV1,
}

impl InteractionInitialResponseResultV1 {
    pub fn intent_digest(&self) -> &InteractionInitialResponseIntentDigestV1 {
        &self.intent_digest
    }

    pub const fn result(&self) -> InteractionInitialResponseResultKindV1 {
        self.result
    }

    pub fn digest(&self) -> &InteractionInitialResponseResultDigestV1 {
        &self.digest
    }
}

impl Debug for InteractionInitialResponseResultV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionInitialResponseResultV1(<redacted>)")
    }
}

#[allow(async_fn_in_trait)]
pub trait InteractionEffectPermitV1 {
    type Error;

    async fn commit_initial_response_intent_v1(
        &self,
        intent: &InteractionInitialResponseIntentV1,
    ) -> Result<InteractionInitialResponseIntentDispositionV1, Self::Error>;

    async fn commit_initial_response_result_v1(
        &self,
        result: &InteractionInitialResponseResultV1,
    ) -> Result<(), Self::Error>;

    async fn commit_idempotent_execution_intent_v1(&self) -> Result<(), Self::Error>;
}

pub struct ReceiptFencedInteractionResponderV1<'a, R: ?Sized, P: ?Sized> {
    responder: &'a R,
    permit: &'a P,
}

impl<'a, R: ?Sized, P: ?Sized> ReceiptFencedInteractionResponderV1<'a, R, P> {
    pub const fn new(responder: &'a R, permit: &'a P) -> Self {
        Self { responder, permit }
    }
}

impl<R: ?Sized, P: ?Sized> Debug for ReceiptFencedInteractionResponderV1<'_, R, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptFencedInteractionResponderV1(<redacted>)")
    }
}

impl<R, P> InteractionResponder for ReceiptFencedInteractionResponderV1<'_, R, P>
where
    R: InteractionResponder + ?Sized,
    P: InteractionEffectPermitV1 + ?Sized,
{
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        let operation = encode_initial_response_operation_v1(InitialResponsePayloadV1::Respond(
            content.as_str(),
        ));
        let intent = build_initial_response_intent_v1(
            InteractionInitialResponseKindV1::RespondEphemeral,
            &operation,
        );
        if !persist_initial_response_intent_v1(self.permit, &intent)
            .await?
            .external_call_authorized()
        {
            return Ok(());
        }
        let external = self.responder.respond_ephemeral(content).await;
        finish_initial_response_v1(self.permit, intent, &operation, external).await
    }

    async fn open_modal(&self, modal: &ModalPresentation) -> Result<(), AdapterError> {
        let operation =
            encode_initial_response_operation_v1(InitialResponsePayloadV1::Modal(modal));
        let intent = build_initial_response_intent_v1(
            InteractionInitialResponseKindV1::OpenModal,
            &operation,
        );
        if !persist_initial_response_intent_v1(self.permit, &intent)
            .await?
            .external_call_authorized()
        {
            return Ok(());
        }
        let external = self.responder.open_modal(modal).await;
        finish_initial_response_v1(self.permit, intent, &operation, external).await
    }

    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        let operation =
            encode_initial_response_operation_v1(InitialResponsePayloadV1::DeferEphemeral);
        let intent = build_initial_response_intent_v1(
            InteractionInitialResponseKindV1::DeferEphemeral,
            &operation,
        );
        if !persist_initial_response_intent_v1(self.permit, &intent)
            .await?
            .external_call_authorized()
        {
            return Ok(());
        }
        let external = self.responder.defer_ephemeral().await;
        finish_initial_response_v1(self.permit, intent, &operation, external).await
    }

    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.responder
            .edit_response(content)
            .await
            .map_err(sanitize_execution_error_v1)
    }
}

pub struct ReceiptFencedDiscordMutationAdapterV1<'a, M: ?Sized, P: ?Sized> {
    mutation: &'a M,
    permit: &'a P,
}

impl<'a, M: ?Sized, P: ?Sized> ReceiptFencedDiscordMutationAdapterV1<'a, M, P> {
    pub const fn new(mutation: &'a M, permit: &'a P) -> Self {
        Self { mutation, permit }
    }
}

impl<M: ?Sized, P: ?Sized> Debug for ReceiptFencedDiscordMutationAdapterV1<'_, M, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptFencedDiscordMutationAdapterV1(<redacted>)")
    }
}

impl<M, P> DiscordMutationAdapter for ReceiptFencedDiscordMutationAdapterV1<'_, M, P>
where
    M: DiscordMutationAdapter + ?Sized,
    P: InteractionEffectPermitV1 + ?Sized,
{
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.mutation
            .grant_role(guild, member, role)
            .await
            .map_err(sanitize_execution_error_v1)
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.mutation
            .create_channel(guild, spec)
            .await
            .map_err(sanitize_execution_error_v1)
    }

    async fn create_role(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.mutation
            .create_role(guild, spec)
            .await
            .map_err(sanitize_execution_error_v1)
    }

    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.mutation
            .upsert_overwrite(guild, channel, target, allow, deny)
            .await
            .map_err(sanitize_execution_error_v1)
    }

    async fn post_panel(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        persist_execution_intent_v1(self.permit).await?;
        self.mutation
            .post_panel(guild, channel, spec)
            .await
            .map_err(sanitize_execution_error_v1)
    }
}

pub struct ReceiptFencedInstanceRegistrarV1<'a, S: ?Sized, P: ?Sized> {
    registrar: &'a S,
    permit: &'a P,
}

impl<'a, S: ?Sized, P: ?Sized> ReceiptFencedInstanceRegistrarV1<'a, S, P> {
    pub const fn new(registrar: &'a S, permit: &'a P) -> Self {
        Self { registrar, permit }
    }
}

impl<S: ?Sized, P: ?Sized> Debug for ReceiptFencedInstanceRegistrarV1<'_, S, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptFencedInstanceRegistrarV1(<redacted>)")
    }
}

impl<S, P> InstanceRegistrarV1 for ReceiptFencedInstanceRegistrarV1<'_, S, P>
where
    S: InstanceRegistrarV1 + ?Sized,
    P: InteractionEffectPermitV1 + ?Sized,
{
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError> {
        self.permit
            .commit_idempotent_execution_intent_v1()
            .await
            .map_err(|_| instance_registration_receipt_persistence_error_v1())?;
        self.registrar.register_instance_v1(instance).await
    }
}

pub(crate) struct ReceiptFencedInstanceTeardownServiceV1<'a, T: ?Sized, P: ?Sized> {
    teardown: &'a T,
    permit: &'a P,
}

impl<'a, T: ?Sized, P: ?Sized> ReceiptFencedInstanceTeardownServiceV1<'a, T, P> {
    pub(crate) const fn new(teardown: &'a T, permit: &'a P) -> Self {
        Self { teardown, permit }
    }
}

impl<T: ?Sized, P: ?Sized> Debug for ReceiptFencedInstanceTeardownServiceV1<'_, T, P> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptFencedInstanceTeardownServiceV1(<redacted>)")
    }
}

impl<T, P> InstanceTeardownService for ReceiptFencedInstanceTeardownServiceV1<'_, T, P>
where
    T: InstanceTeardownService + ?Sized,
    P: InteractionEffectPermitV1 + ?Sized,
{
    async fn teardown(
        &self,
        guild_id: GuildId,
        instance_id: InstanceId,
    ) -> Result<TeardownOutcome, TeardownError> {
        self.permit
            .commit_idempotent_execution_intent_v1()
            .await
            .map_err(|_| teardown_receipt_persistence_error_v1())?;
        self.teardown.teardown(guild_id, instance_id).await
    }
}

async fn persist_initial_response_intent_v1<P: InteractionEffectPermitV1 + ?Sized>(
    permit: &P,
    intent: &InteractionInitialResponseIntentV1,
) -> Result<InteractionInitialResponseIntentDispositionV1, AdapterError> {
    permit
        .commit_initial_response_intent_v1(intent)
        .await
        .map_err(|_| receipt_persistence_error_v1())
}

async fn finish_initial_response_v1<P: InteractionEffectPermitV1 + ?Sized>(
    permit: &P,
    intent: InteractionInitialResponseIntentV1,
    operation: &[u8],
    external: Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    let result_kind = classify_initial_response_result_v1(&external);
    let result = build_initial_response_result_v1(intent.digest, result_kind, operation);
    permit
        .commit_initial_response_result_v1(&result)
        .await
        .map_err(|_| receipt_persistence_error_v1())?;
    external.map_err(sanitize_initial_response_error_v1)
}

async fn persist_execution_intent_v1<P: InteractionEffectPermitV1 + ?Sized>(
    permit: &P,
) -> Result<(), AdapterError> {
    permit
        .commit_idempotent_execution_intent_v1()
        .await
        .map_err(|_| receipt_persistence_error_v1())
}

fn classify_initial_response_result_v1(
    external: &Result<(), AdapterError>,
) -> InteractionInitialResponseResultKindV1 {
    match external {
        Ok(()) => InteractionInitialResponseResultKindV1::Succeeded,
        Err(error) => match &error.kind {
            AdapterErrorKind::Network | AdapterErrorKind::Unknown => {
                InteractionInitialResponseResultKindV1::Indeterminate
            }
            AdapterErrorKind::Forbidden
            | AdapterErrorKind::NotFound
            | AdapterErrorKind::RateLimited
            | AdapterErrorKind::Unsupported
            | AdapterErrorKind::BadRequest
            | AdapterErrorKind::InvalidEventRoute => {
                InteractionInitialResponseResultKindV1::DefinitiveFailure
            }
        },
    }
}

fn receipt_persistence_error_v1() -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::Unknown,
        RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1,
    )
}

fn teardown_receipt_persistence_error_v1() -> TeardownError {
    TeardownError::Store(InstanceStoreError::Backend(
        TEARDOWN_RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1.to_string(),
    ))
}

fn instance_registration_receipt_persistence_error_v1() -> InstanceStoreError {
    InstanceStoreError::Backend(
        INSTANCE_REGISTRATION_RECEIPT_PERSISTENCE_FAILURE_MESSAGE_V1.to_string(),
    )
}

fn sanitize_initial_response_error_v1(error: AdapterError) -> AdapterError {
    AdapterError::new(error.kind, INITIAL_RESPONSE_FAILURE_MESSAGE_V1)
}

fn sanitize_execution_error_v1(error: AdapterError) -> AdapterError {
    AdapterError::new(error.kind, EXECUTION_FAILURE_MESSAGE_V1)
}

enum InitialResponsePayloadV1<'a> {
    Respond(&'a str),
    Modal(&'a ModalPresentation),
    DeferEphemeral,
}

fn encode_initial_response_operation_v1(payload: InitialResponsePayloadV1<'_>) -> Vec<u8> {
    let mut frame = CanonicalFrameV1::new(INITIAL_RESPONSE_OPERATION_DOMAIN_V1);
    match payload {
        InitialResponsePayloadV1::Respond(content) => {
            frame.u8(3, 1);
            frame.text(4, content);
        }
        InitialResponsePayloadV1::Modal(modal) => {
            frame.u8(3, 2);
            frame.nested(4, encode_modal_v1(modal));
        }
        InitialResponsePayloadV1::DeferEphemeral => frame.u8(3, 3),
    }
    frame.finish()
}

fn build_initial_response_intent_v1(
    kind: InteractionInitialResponseKindV1,
    operation: &[u8],
) -> InteractionInitialResponseIntentV1 {
    let mut frame = CanonicalFrameV1::new(INITIAL_RESPONSE_INTENT_DOMAIN_V1);
    frame.bytes(3, operation);
    InteractionInitialResponseIntentV1 {
        kind,
        digest: InteractionInitialResponseIntentDigestV1::from_canonical_bytes(&frame.finish()),
    }
}

fn build_initial_response_result_v1(
    intent_digest: InteractionInitialResponseIntentDigestV1,
    result: InteractionInitialResponseResultKindV1,
    operation: &[u8],
) -> InteractionInitialResponseResultV1 {
    let mut frame = CanonicalFrameV1::new(INITIAL_RESPONSE_RESULT_DOMAIN_V1);
    frame.bytes(3, operation);
    frame.bytes(4, intent_digest.as_bytes());
    frame.u8(
        5,
        match result {
            InteractionInitialResponseResultKindV1::Succeeded => 1,
            InteractionInitialResponseResultKindV1::DefinitiveFailure => 2,
            InteractionInitialResponseResultKindV1::Indeterminate => 3,
        },
    );
    InteractionInitialResponseResultV1 {
        intent_digest,
        result,
        digest: InteractionInitialResponseResultDigestV1::from_canonical_bytes(&frame.finish()),
    }
}

fn encode_modal_v1(modal: &ModalPresentation) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MODAL_DOMAIN_V1);
    frame.text(3, &modal.key);
    frame.text(4, &modal.title);
    frame.u64(5, usize_to_u64_v1(modal.fields.len()));
    for field in &modal.fields {
        frame.nested(6, encode_modal_field_v1(field));
    }
    frame
}

fn encode_modal_field_v1(field: &ModalFieldSpec) -> CanonicalFrameV1 {
    let mut frame = CanonicalFrameV1::new(MODAL_FIELD_DOMAIN_V1);
    frame.text(3, &field.key);
    frame.text(4, &field.label);
    frame.u8(
        5,
        match field.style {
            ModalFieldStyle::Short => 1,
            ModalFieldStyle::Paragraph => 2,
        },
    );
    frame.u8(6, u8::from(field.required));
    frame.optional_u16(7, 8, field.min_length);
    frame.optional_u16(9, 10, field.max_length);
    frame.u8(
        11,
        match field.input_policy {
            ModalInputPolicy::Preserve => 1,
            ModalInputPolicy::TrimUnicodeWhitespace => 2,
        },
    );
    frame
}

fn usize_to_u64_v1(value: usize) -> u64 {
    u64::try_from(value).expect("collection length fits the canonical u64 frame")
}

struct CanonicalFrameV1 {
    bytes: Vec<u8>,
}

impl CanonicalFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut frame = Self {
            bytes: Vec::with_capacity(256),
        };
        frame.bytes(1, domain);
        frame.u16(2, CANONICAL_VERSION_V1);
        frame
    }

    fn bytes(&mut self, tag: u16, value: &[u8]) {
        self.bytes.extend_from_slice(&tag.to_be_bytes());
        self.bytes
            .extend_from_slice(&usize_to_u64_v1(value.len()).to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn text(&mut self, tag: u16, value: &str) {
        self.bytes(tag, value.as_bytes());
    }

    fn u8(&mut self, tag: u16, value: u8) {
        self.bytes(tag, &[value]);
    }

    fn u16(&mut self, tag: u16, value: u16) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn u64(&mut self, tag: u16, value: u64) {
        self.bytes(tag, &value.to_be_bytes());
    }

    fn optional_u16(&mut self, discriminant_tag: u16, value_tag: u16, value: Option<u16>) {
        self.u8(discriminant_tag, u8::from(value.is_some()));
        if let Some(value) = value {
            self.u16(value_tag, value);
        }
    }

    fn nested(&mut self, tag: u16, value: CanonicalFrameV1) {
        self.bytes(tag, &value.finish());
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests;
