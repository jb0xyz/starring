#[cfg(test)]
use std::collections::BTreeSet;
use std::fmt::{Debug, Display, Formatter};

use automation_instance::{InstanceId, InstanceKind, InstanceRuleSetVersion};
use automation_runtime_interaction::{
    InteractionEffectActionIndexV1, InteractionEffectChannelIdV1, InteractionEffectKindV1,
    InteractionEffectMessageIdV1, InteractionEffectPlannedDependencyV1, InteractionEffectRoleIdV1,
    InteractionEffectUserIdV1,
};
#[cfg(test)]
use automation_runtime_interaction::{
    InteractionEffectOutputClassV1, InteractionEffectPlannedTargetV1,
};
#[cfg(test)]
use sha2::{Digest, Sha256};
use zeroize::Zeroize;
#[cfg(test)]
use zeroize::Zeroizing;

#[cfg(test)]
use crate::InteractionEffectJournalPlanEntryV1;
use crate::{InteractionActionPreflightCertificateV1, InteractionEffectJournalPlanV1};

#[cfg(test)]
const DISPATCH_BODY_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.effect.dispatch.body.v1\0";
#[cfg(test)]
const DISPATCH_ACTION_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.dispatch.action.v1\0";
#[cfg(test)]
const PREPARED_DISPATCH_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.effect.dispatch.prepared.v1\0";
#[cfg(test)]
const CANONICAL_VERSION_V1: u16 = 1;

#[cfg(test)]
const MAX_SOURCE_NODE_ID_BYTES_V1: usize = 128;
#[cfg(test)]
const MAX_RESOURCE_ALIAS_BYTES_V1: usize = 64;
#[cfg(test)]
const MAX_RULESET_KEY_BYTES_V1: usize = 128;
#[cfg(test)]
const MAX_INSTANCE_KIND_BYTES_V1: usize = 128;
#[cfg(test)]
const MAX_DISCORD_NAME_UTF16_V1: usize = 100;
#[cfg(test)]
const MAX_RESPONSE_CONTENT_UTF16_V1: usize = 2_000;
#[cfg(test)]
const MAX_BUTTON_LABEL_UTF16_V1: usize = 80;
#[cfg(test)]
const MAX_CUSTOM_ID_BYTES_V1: usize = 100;
#[cfg(test)]
const MAX_PANEL_BUTTONS_V1: usize = 25;
#[cfg(test)]
const MAX_MANIFEST_ROLES_V1: usize = 250;
#[cfg(test)]
const MAX_MANIFEST_CHANNELS_V1: usize = 500;
#[cfg(test)]
const MAX_MANIFEST_MESSAGES_V1: usize = 1_000;

pub const MAX_INTERACTION_EFFECT_DISPATCH_CANONICAL_BYTES_V1: usize = 1_048_576;

/// This milestone deliberately exposes no stateful or live dispatch constructor.
pub const STATEFUL_INTERACTION_EFFECT_DISPATCH_INTEGRATED_V1: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectDispatchReceiptRequirementV1 {
    /// The deferred acknowledgement and the complete journal plan must be committed together
    /// before the first external call. A failed tail must also be recorded durably.
    DeferredEphemeralSuccessAndAtomicJournalPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "stateful interaction effect dispatch is unavailable until deferred receipt, journal plan, and failure-tail records can be committed atomically"
)]
pub struct StatefulInteractionEffectDispatchUnavailableV1;

pub fn require_stateful_interaction_effect_dispatch_integration_v1(
) -> Result<(), StatefulInteractionEffectDispatchUnavailableV1> {
    Err(StatefulInteractionEffectDispatchUnavailableV1)
}

macro_rules! define_output_digest {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            #[cfg(test)]
            fn from_canonical_v1(canonical: &[u8]) -> Self {
                Self(lower_hex_v1(&Sha256::digest(canonical)))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

define_output_digest!(InteractionEffectDispatchBodyDigestV1);
define_output_digest!(InteractionEffectDispatchActionDigestV1);
define_output_digest!(InteractionEffectDispatchPreparedDigestV1);

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchSourceIdentityV1 {
    node_id: String,
    source_ordinal: u16,
}

impl InteractionEffectDispatchSourceIdentityV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn source_ordinal(&self) -> u16 {
        self.source_ordinal
    }

    #[cfg(test)]
    pub(crate) fn from_test_scaffold_v1(
        node_id: String,
        source_ordinal: u16,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_source_node_id_v1(&node_id)?;
        Ok(Self {
            node_id,
            source_ordinal,
        })
    }
}

impl Debug for InteractionEffectDispatchSourceIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectDispatchSourceIdentityV1")
            .field("node_id", &"<redacted>")
            .field("source_ordinal", &self.source_ordinal)
            .finish()
    }
}

impl Drop for InteractionEffectDispatchSourceIdentityV1 {
    fn drop(&mut self) {
        self.node_id.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchButtonV1 {
    index: u8,
    label: String,
    custom_id: String,
}

impl InteractionEffectDispatchButtonV1 {
    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }
}

impl Debug for InteractionEffectDispatchButtonV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchButtonV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchButtonV1 {
    fn drop(&mut self) {
        self.label.zeroize();
        self.custom_id.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchResourceDependencyV1 {
    alias: String,
    dependency: InteractionEffectPlannedDependencyV1,
}

impl InteractionEffectDispatchResourceDependencyV1 {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn dependency(&self) -> &InteractionEffectPlannedDependencyV1 {
        &self.dependency
    }
}

impl Debug for InteractionEffectDispatchResourceDependencyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchResourceDependencyV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchResourceDependencyV1 {
    fn drop(&mut self) {
        self.alias.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchRoleResourceV1 {
    alias: String,
    role_id: InteractionEffectRoleIdV1,
}

impl InteractionEffectDispatchRoleResourceV1 {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn role_id(&self) -> InteractionEffectRoleIdV1 {
        self.role_id
    }
}

impl Debug for InteractionEffectDispatchRoleResourceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchRoleResourceV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchRoleResourceV1 {
    fn drop(&mut self) {
        self.alias.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchChannelResourceV1 {
    alias: String,
    channel_id: InteractionEffectChannelIdV1,
}

impl InteractionEffectDispatchChannelResourceV1 {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn channel_id(&self) -> InteractionEffectChannelIdV1 {
        self.channel_id
    }
}

impl Debug for InteractionEffectDispatchChannelResourceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchChannelResourceV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchChannelResourceV1 {
    fn drop(&mut self) {
        self.alias.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchMessageResourceV1 {
    alias: String,
    channel_id: InteractionEffectChannelIdV1,
    message_id: InteractionEffectMessageIdV1,
}

impl InteractionEffectDispatchMessageResourceV1 {
    pub fn alias(&self) -> &str {
        &self.alias
    }

    pub fn channel_id(&self) -> InteractionEffectChannelIdV1 {
        self.channel_id
    }

    pub fn message_id(&self) -> InteractionEffectMessageIdV1 {
        self.message_id
    }
}

impl Debug for InteractionEffectDispatchMessageResourceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchMessageResourceV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchMessageResourceV1 {
    fn drop(&mut self) {
        self.alias.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchRegistrationIdentityV1 {
    ruleset_key: String,
    ruleset_version: InstanceRuleSetVersion,
    kind: InstanceKind,
    created_by: InteractionEffectUserIdV1,
}

impl InteractionEffectDispatchRegistrationIdentityV1 {
    pub fn ruleset_key(&self) -> &str {
        &self.ruleset_key
    }

    pub fn ruleset_version(&self) -> InstanceRuleSetVersion {
        self.ruleset_version
    }

    pub fn kind(&self) -> &InstanceKind {
        &self.kind
    }

    pub fn created_by(&self) -> InteractionEffectUserIdV1 {
        self.created_by
    }
}

impl Debug for InteractionEffectDispatchRegistrationIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InteractionEffectDispatchRegistrationIdentityV1(<redacted>)")
    }
}

impl Drop for InteractionEffectDispatchRegistrationIdentityV1 {
    fn drop(&mut self) {
        self.ruleset_key.zeroize();
        self.kind.0.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchTeardownManifestV1 {
    instance_id: InstanceId,
    registration: InteractionEffectDispatchRegistrationIdentityV1,
    roles: Vec<InteractionEffectDispatchRoleResourceV1>,
    channels: Vec<InteractionEffectDispatchChannelResourceV1>,
    messages: Vec<InteractionEffectDispatchMessageResourceV1>,
}

impl InteractionEffectDispatchTeardownManifestV1 {
    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn registration(&self) -> &InteractionEffectDispatchRegistrationIdentityV1 {
        &self.registration
    }

    pub fn roles(&self) -> &[InteractionEffectDispatchRoleResourceV1] {
        &self.roles
    }

    pub fn channels(&self) -> &[InteractionEffectDispatchChannelResourceV1] {
        &self.channels
    }

    pub fn messages(&self) -> &[InteractionEffectDispatchMessageResourceV1] {
        &self.messages
    }
}

impl Debug for InteractionEffectDispatchTeardownManifestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectDispatchTeardownManifestV1")
            .field("instance_id", &"<redacted>")
            .field("role_count", &self.roles.len())
            .field("channel_count", &self.channels.len())
            .field("message_count", &self.messages.len())
            .finish()
    }
}

#[allow(dead_code)]
#[derive(Clone, PartialEq, Eq)]
enum InteractionEffectDispatchBodyInnerV1 {
    CreateRole {
        output_alias: String,
        name: String,
    },
    CreateChannel {
        output_alias: String,
        name: String,
    },
    GrantRole,
    UpsertOverwrite,
    PostPanel {
        output_alias: String,
        content: String,
        buttons: Vec<InteractionEffectDispatchButtonV1>,
    },
    RegisterInstance {
        output_alias: String,
        instance_id: InstanceId,
        registration: InteractionEffectDispatchRegistrationIdentityV1,
        roles: Vec<InteractionEffectDispatchResourceDependencyV1>,
        channels: Vec<InteractionEffectDispatchResourceDependencyV1>,
        messages: Vec<InteractionEffectDispatchResourceDependencyV1>,
    },
    TeardownInstance {
        manifest: InteractionEffectDispatchTeardownManifestV1,
    },
    EditResponse {
        content: String,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub struct InteractionEffectDispatchBodyV1 {
    inner: InteractionEffectDispatchBodyInnerV1,
}

#[derive(Clone, Copy)]
pub enum InteractionEffectDispatchBodyRefV1<'a> {
    CreateRole {
        output_alias: &'a str,
        name: &'a str,
    },
    CreateChannel {
        output_alias: &'a str,
        name: &'a str,
    },
    GrantRole,
    UpsertOverwrite,
    PostPanel {
        output_alias: &'a str,
        content: &'a str,
        buttons: &'a [InteractionEffectDispatchButtonV1],
    },
    RegisterInstance {
        output_alias: &'a str,
        instance_id: &'a InstanceId,
        registration: &'a InteractionEffectDispatchRegistrationIdentityV1,
        roles: &'a [InteractionEffectDispatchResourceDependencyV1],
        channels: &'a [InteractionEffectDispatchResourceDependencyV1],
        messages: &'a [InteractionEffectDispatchResourceDependencyV1],
    },
    TeardownInstance {
        manifest: &'a InteractionEffectDispatchTeardownManifestV1,
    },
    EditResponse {
        content: &'a str,
    },
}

impl InteractionEffectDispatchBodyV1 {
    pub fn kind(&self) -> InteractionEffectKindV1 {
        match self.inner {
            InteractionEffectDispatchBodyInnerV1::CreateRole { .. } => {
                InteractionEffectKindV1::CreateRole
            }
            InteractionEffectDispatchBodyInnerV1::CreateChannel { .. } => {
                InteractionEffectKindV1::CreateChannel
            }
            InteractionEffectDispatchBodyInnerV1::GrantRole => InteractionEffectKindV1::GrantRole,
            InteractionEffectDispatchBodyInnerV1::UpsertOverwrite => {
                InteractionEffectKindV1::UpsertOverwrite
            }
            InteractionEffectDispatchBodyInnerV1::PostPanel { .. } => {
                InteractionEffectKindV1::PostPanel
            }
            InteractionEffectDispatchBodyInnerV1::RegisterInstance { .. } => {
                InteractionEffectKindV1::RegisterInstance
            }
            InteractionEffectDispatchBodyInnerV1::TeardownInstance { .. } => {
                InteractionEffectKindV1::TeardownInstance
            }
            InteractionEffectDispatchBodyInnerV1::EditResponse { .. } => {
                InteractionEffectKindV1::EditResponse
            }
        }
    }

    pub fn view(&self) -> InteractionEffectDispatchBodyRefV1<'_> {
        match &self.inner {
            InteractionEffectDispatchBodyInnerV1::CreateRole { output_alias, name } => {
                InteractionEffectDispatchBodyRefV1::CreateRole { output_alias, name }
            }
            InteractionEffectDispatchBodyInnerV1::CreateChannel { output_alias, name } => {
                InteractionEffectDispatchBodyRefV1::CreateChannel { output_alias, name }
            }
            InteractionEffectDispatchBodyInnerV1::GrantRole => {
                InteractionEffectDispatchBodyRefV1::GrantRole
            }
            InteractionEffectDispatchBodyInnerV1::UpsertOverwrite => {
                InteractionEffectDispatchBodyRefV1::UpsertOverwrite
            }
            InteractionEffectDispatchBodyInnerV1::PostPanel {
                output_alias,
                content,
                buttons,
            } => InteractionEffectDispatchBodyRefV1::PostPanel {
                output_alias,
                content,
                buttons,
            },
            InteractionEffectDispatchBodyInnerV1::RegisterInstance {
                output_alias,
                instance_id,
                registration,
                roles,
                channels,
                messages,
            } => InteractionEffectDispatchBodyRefV1::RegisterInstance {
                output_alias,
                instance_id,
                registration,
                roles,
                channels,
                messages,
            },
            InteractionEffectDispatchBodyInnerV1::TeardownInstance { manifest } => {
                InteractionEffectDispatchBodyRefV1::TeardownInstance { manifest }
            }
            InteractionEffectDispatchBodyInnerV1::EditResponse { content } => {
                InteractionEffectDispatchBodyRefV1::EditResponse { content }
            }
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn create_role_v1(
        output_alias: String,
        name: String,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_alias_v1(&output_alias)?;
        validate_discord_name_v1(&name)?;
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::CreateRole { output_alias, name },
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn create_channel_v1(
        output_alias: String,
        name: String,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_alias_v1(&output_alias)?;
        validate_discord_name_v1(&name)?;
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::CreateChannel { output_alias, name },
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn grant_role_v1() -> Self {
        Self {
            inner: InteractionEffectDispatchBodyInnerV1::GrantRole,
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn upsert_overwrite_v1() -> Self {
        Self {
            inner: InteractionEffectDispatchBodyInnerV1::UpsertOverwrite,
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn post_panel_v1(
        output_alias: String,
        content: String,
        buttons: Vec<InteractionEffectDispatchButtonV1>,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_alias_v1(&output_alias)?;
        validate_content_v1(&content)?;
        if buttons.len() > MAX_PANEL_BUTTONS_V1
            || buttons
                .iter()
                .enumerate()
                .any(|(index, button)| usize::from(button.index) != index)
        {
            return Err(InteractionEffectDispatchBuildErrorV1::Button);
        }
        for button in &buttons {
            validate_utf16_text_v1(&button.label, 1, MAX_BUTTON_LABEL_UTF16_V1)
                .map_err(|_| InteractionEffectDispatchBuildErrorV1::Button)?;
            if button.custom_id.is_empty()
                || button.custom_id.len() > MAX_CUSTOM_ID_BYTES_V1
                || button.custom_id.as_bytes().contains(&0)
            {
                return Err(InteractionEffectDispatchBuildErrorV1::Button);
            }
        }
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::PostPanel {
                output_alias,
                content,
                buttons,
            },
        })
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn register_instance_v1(
        output_alias: String,
        instance_id: InstanceId,
        registration: InteractionEffectDispatchRegistrationIdentityV1,
        roles: Vec<InteractionEffectDispatchResourceDependencyV1>,
        channels: Vec<InteractionEffectDispatchResourceDependencyV1>,
        messages: Vec<InteractionEffectDispatchResourceDependencyV1>,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_alias_v1(&output_alias)?;
        validate_registration_v1(&registration)?;
        validate_dependency_resources_v1(
            &roles,
            MAX_MANIFEST_ROLES_V1,
            InteractionEffectOutputClassV1::CreatedRole,
        )?;
        validate_dependency_resources_v1(
            &channels,
            MAX_MANIFEST_CHANNELS_V1,
            InteractionEffectOutputClassV1::CreatedChannel,
        )?;
        validate_dependency_resources_v1(
            &messages,
            MAX_MANIFEST_MESSAGES_V1,
            InteractionEffectOutputClassV1::PostedMessage,
        )?;
        let resource_count = roles
            .len()
            .checked_add(channels.len())
            .and_then(|count| count.checked_add(messages.len()))
            .ok_or(InteractionEffectDispatchBuildErrorV1::Resources)?;
        if resource_count != registration_dependency_count_v1(&roles, &channels, &messages) {
            return Err(InteractionEffectDispatchBuildErrorV1::Resources);
        }
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::RegisterInstance {
                output_alias,
                instance_id,
                registration,
                roles,
                channels,
                messages,
            },
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn teardown_instance_v1(
        manifest: InteractionEffectDispatchTeardownManifestV1,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_teardown_manifest_v1(&manifest)?;
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::TeardownInstance { manifest },
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn edit_response_v1(
        content: String,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        validate_content_v1(&content)?;
        Ok(Self {
            inner: InteractionEffectDispatchBodyInnerV1::EditResponse { content },
        })
    }
}

impl Debug for InteractionEffectDispatchBodyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectDispatchBodyV1")
            .field("kind", &self.kind())
            .field("contents", &"<redacted>")
            .finish()
    }
}

impl Drop for InteractionEffectDispatchBodyV1 {
    fn drop(&mut self) {
        match &mut self.inner {
            InteractionEffectDispatchBodyInnerV1::CreateRole { output_alias, name }
            | InteractionEffectDispatchBodyInnerV1::CreateChannel { output_alias, name } => {
                output_alias.zeroize();
                name.zeroize();
            }
            InteractionEffectDispatchBodyInnerV1::PostPanel {
                output_alias,
                content,
                ..
            } => {
                output_alias.zeroize();
                content.zeroize();
            }
            InteractionEffectDispatchBodyInnerV1::RegisterInstance { output_alias, .. } => {
                output_alias.zeroize();
            }
            InteractionEffectDispatchBodyInnerV1::EditResponse { content } => content.zeroize(),
            InteractionEffectDispatchBodyInnerV1::GrantRole
            | InteractionEffectDispatchBodyInnerV1::UpsertOverwrite
            | InteractionEffectDispatchBodyInnerV1::TeardownInstance { .. } => {}
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct InteractionEffectDispatchActionV1 {
    source: InteractionEffectDispatchSourceIdentityV1,
    effect_cursor: InteractionEffectActionIndexV1,
    body: InteractionEffectDispatchBodyV1,
    body_digest: InteractionEffectDispatchBodyDigestV1,
    action_digest: InteractionEffectDispatchActionDigestV1,
}

impl InteractionEffectDispatchActionV1 {
    pub fn source(&self) -> &InteractionEffectDispatchSourceIdentityV1 {
        &self.source
    }

    pub fn effect_cursor(&self) -> InteractionEffectActionIndexV1 {
        self.effect_cursor
    }

    pub fn body(&self) -> &InteractionEffectDispatchBodyV1 {
        &self.body
    }

    pub fn body_digest(&self) -> &InteractionEffectDispatchBodyDigestV1 {
        &self.body_digest
    }

    pub fn action_digest(&self) -> &InteractionEffectDispatchActionDigestV1 {
        &self.action_digest
    }
}

impl Debug for InteractionEffectDispatchActionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InteractionEffectDispatchActionV1")
            .field("effect_cursor", &self.effect_cursor)
            .field("kind", &self.body.kind())
            .field("contents", &"<redacted>")
            .finish()
    }
}

#[derive(PartialEq, Eq)]
pub struct PreparedInteractionEffectDispatchV1 {
    certificate: InteractionActionPreflightCertificateV1,
    journal_plan: InteractionEffectJournalPlanV1,
    actions: Vec<InteractionEffectDispatchActionV1>,
    digest: InteractionEffectDispatchPreparedDigestV1,
}

impl PreparedInteractionEffectDispatchV1 {
    pub fn certificate(&self) -> &InteractionActionPreflightCertificateV1 {
        &self.certificate
    }

    pub fn journal_plan(&self) -> &InteractionEffectJournalPlanV1 {
        &self.journal_plan
    }

    pub fn actions(&self) -> &[InteractionEffectDispatchActionV1] {
        &self.actions
    }

    pub fn digest(&self) -> &InteractionEffectDispatchPreparedDigestV1 {
        &self.digest
    }

    pub const fn receipt_requirement(&self) -> InteractionEffectDispatchReceiptRequirementV1 {
        InteractionEffectDispatchReceiptRequirementV1::DeferredEphemeralSuccessAndAtomicJournalPlan
    }

    #[cfg(test)]
    pub(crate) fn from_test_scaffold_v1(
        certificate: InteractionActionPreflightCertificateV1,
        journal_plan: InteractionEffectJournalPlanV1,
        actions: Vec<(
            InteractionEffectDispatchSourceIdentityV1,
            InteractionEffectDispatchBodyV1,
        )>,
    ) -> Result<Self, InteractionEffectDispatchBuildErrorV1> {
        if journal_plan.preflight_certificate_digest() != certificate.digest()
            || journal_plan.snapshot_digest() != certificate.snapshot_digest()
        {
            return Err(InteractionEffectDispatchBuildErrorV1::CertificateBinding);
        }
        if actions.is_empty() || actions.len() != journal_plan.entries().len() {
            return Err(InteractionEffectDispatchBuildErrorV1::ActionCount);
        }

        let mut source_ids = BTreeSet::new();
        let mut previous_ordinal = None;
        let mut prepared_actions = Vec::with_capacity(actions.len());
        for ((source, body), journal_entry) in actions.into_iter().zip(journal_plan.entries()) {
            if previous_ordinal.is_some_and(|previous| previous >= source.source_ordinal)
                || !source_ids.insert(source.node_id.clone())
            {
                return Err(InteractionEffectDispatchBuildErrorV1::SourceOrder);
            }
            previous_ordinal = Some(source.source_ordinal);
            validate_body_binding_v1(&body, journal_entry)?;
            let body_canonical = encode_body_v1(&body);
            if body_canonical.len() > MAX_INTERACTION_EFFECT_DISPATCH_CANONICAL_BYTES_V1 {
                return Err(InteractionEffectDispatchBuildErrorV1::CanonicalSize);
            }
            let body_digest =
                InteractionEffectDispatchBodyDigestV1::from_canonical_v1(&body_canonical);
            let effect_cursor = journal_entry.definition().action().action_index();
            let action_canonical =
                encode_action_v1(&source, effect_cursor, journal_entry, &body_canonical);
            let action_digest =
                InteractionEffectDispatchActionDigestV1::from_canonical_v1(&action_canonical);
            prepared_actions.push(InteractionEffectDispatchActionV1 {
                source,
                effect_cursor,
                body,
                body_digest,
                action_digest,
            });
        }

        let canonical = encode_prepared_v1(&certificate, &journal_plan, &prepared_actions);
        if canonical.len() > MAX_INTERACTION_EFFECT_DISPATCH_CANONICAL_BYTES_V1 {
            return Err(InteractionEffectDispatchBuildErrorV1::CanonicalSize);
        }
        let digest = InteractionEffectDispatchPreparedDigestV1::from_canonical_v1(&canonical);
        Ok(Self {
            certificate,
            journal_plan,
            actions: prepared_actions,
            digest,
        })
    }
}

impl Debug for PreparedInteractionEffectDispatchV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedInteractionEffectDispatchV1")
            .field("effect_count", &self.actions.len())
            .field("contents", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum InteractionEffectDispatchBuildErrorV1 {
    #[error("interaction effect dispatch source identity is invalid")]
    SourceIdentity,
    #[error("interaction effect dispatch source order is not canonical")]
    SourceOrder,
    #[error("interaction effect dispatch resource alias is invalid")]
    Alias,
    #[error("interaction effect dispatch text is invalid")]
    Text,
    #[error("interaction effect dispatch button is invalid")]
    Button,
    #[error("interaction effect dispatch registration identity is invalid")]
    Registration,
    #[error("interaction effect dispatch resource bindings are invalid")]
    Resources,
    #[error("interaction effect dispatch body does not bind its journal definition")]
    BodyBinding,
    #[error("interaction effect dispatch action count does not match its journal plan")]
    ActionCount,
    #[error("interaction effect dispatch is not bound to its certificate")]
    CertificateBinding,
    #[error("interaction effect dispatch canonical representation is too large")]
    CanonicalSize,
}

#[cfg(test)]
fn validate_source_node_id_v1(value: &str) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    if value.is_empty()
        || value.len() > MAX_SOURCE_NODE_ID_BYTES_V1
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
    {
        return Err(InteractionEffectDispatchBuildErrorV1::SourceIdentity);
    }
    Ok(())
}

#[cfg(test)]
fn validate_alias_v1(value: &str) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    if value.is_empty()
        || value.len() > MAX_RESOURCE_ALIAS_BYTES_V1
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(InteractionEffectDispatchBuildErrorV1::Alias);
    }
    Ok(())
}

#[cfg(test)]
fn validate_utf16_text_v1(
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    let units = value.encode_utf16().count();
    if units < minimum || units > maximum || value.as_bytes().contains(&0) {
        return Err(InteractionEffectDispatchBuildErrorV1::Text);
    }
    Ok(())
}

#[cfg(test)]
fn validate_discord_name_v1(value: &str) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    validate_utf16_text_v1(value, 1, MAX_DISCORD_NAME_UTF16_V1)
}

#[cfg(test)]
fn validate_content_v1(value: &str) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    validate_utf16_text_v1(value, 0, MAX_RESPONSE_CONTENT_UTF16_V1)
}

#[cfg(test)]
fn validate_registration_v1(
    registration: &InteractionEffectDispatchRegistrationIdentityV1,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    if registration.ruleset_key.is_empty()
        || registration.ruleset_key.len() > MAX_RULESET_KEY_BYTES_V1
        || registration.ruleset_key.as_bytes().contains(&0)
        || registration.kind.0.is_empty()
        || registration.kind.0.len() > MAX_INSTANCE_KIND_BYTES_V1
        || registration.kind.0.as_bytes().contains(&0)
    {
        return Err(InteractionEffectDispatchBuildErrorV1::Registration);
    }
    Ok(())
}

#[cfg(test)]
fn validate_dependency_resources_v1(
    resources: &[InteractionEffectDispatchResourceDependencyV1],
    maximum: usize,
    output_class: InteractionEffectOutputClassV1,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    if resources.len() > maximum {
        return Err(InteractionEffectDispatchBuildErrorV1::Resources);
    }
    let mut previous = None;
    for resource in resources {
        validate_alias_v1(&resource.alias)
            .map_err(|_| InteractionEffectDispatchBuildErrorV1::Resources)?;
        if previous.is_some_and(|candidate: &str| candidate >= resource.alias.as_str())
            || resource.dependency.output_class() != output_class
        {
            return Err(InteractionEffectDispatchBuildErrorV1::Resources);
        }
        previous = Some(resource.alias.as_str());
    }
    Ok(())
}

#[cfg(test)]
fn validate_exact_resources_v1<T>(
    resources: &[T],
    maximum: usize,
    alias: impl Fn(&T) -> &str,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    if resources.len() > maximum {
        return Err(InteractionEffectDispatchBuildErrorV1::Resources);
    }
    let mut previous = None;
    for resource in resources {
        let current = alias(resource);
        validate_alias_v1(current).map_err(|_| InteractionEffectDispatchBuildErrorV1::Resources)?;
        if previous.is_some_and(|candidate: &str| candidate >= current) {
            return Err(InteractionEffectDispatchBuildErrorV1::Resources);
        }
        previous = Some(current);
    }
    Ok(())
}

#[cfg(test)]
fn validate_teardown_manifest_v1(
    manifest: &InteractionEffectDispatchTeardownManifestV1,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    validate_registration_v1(&manifest.registration)?;
    validate_exact_resources_v1(&manifest.roles, MAX_MANIFEST_ROLES_V1, |item| {
        item.alias.as_str()
    })?;
    validate_exact_resources_v1(&manifest.channels, MAX_MANIFEST_CHANNELS_V1, |item| {
        item.alias.as_str()
    })?;
    validate_exact_resources_v1(&manifest.messages, MAX_MANIFEST_MESSAGES_V1, |item| {
        item.alias.as_str()
    })?;
    Ok(())
}

#[cfg(test)]
fn validate_body_binding_v1(
    body: &InteractionEffectDispatchBodyV1,
    journal_entry: &InteractionEffectJournalPlanEntryV1,
) -> Result<(), InteractionEffectDispatchBuildErrorV1> {
    let definition = journal_entry.definition();
    if body.kind() != definition.action().kind() {
        return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding);
    }
    match (&body.inner, definition.recovery_input().target()) {
        (
            InteractionEffectDispatchBodyInnerV1::RegisterInstance {
                instance_id,
                registration,
                roles,
                channels,
                messages,
                ..
            },
            InteractionEffectPlannedTargetV1::RegisterInstance { target, kind, .. },
        ) => {
            if instance_id != target.instance_id() || &registration.kind != kind {
                return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding);
            }
            if roles.len() + channels.len() + messages.len() != definition.dependencies().len() {
                return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding);
            }
            for resource in roles.iter().chain(channels).chain(messages) {
                if !definition.dependencies().contains(&resource.dependency) {
                    return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding);
                }
            }
        }
        (
            InteractionEffectDispatchBodyInnerV1::TeardownInstance { manifest },
            InteractionEffectPlannedTargetV1::TeardownInstance { target },
        ) if manifest.instance_id != *target.instance_id() => {
            return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding);
        }
        (
            InteractionEffectDispatchBodyInnerV1::CreateRole { .. },
            InteractionEffectPlannedTargetV1::CreateRole { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::CreateChannel { .. },
            InteractionEffectPlannedTargetV1::CreateChannel { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::GrantRole,
            InteractionEffectPlannedTargetV1::GrantRole { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::UpsertOverwrite,
            InteractionEffectPlannedTargetV1::UpsertOverwrite { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::PostPanel { .. },
            InteractionEffectPlannedTargetV1::PostPanel { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::TeardownInstance { .. },
            InteractionEffectPlannedTargetV1::TeardownInstance { .. },
        )
        | (
            InteractionEffectDispatchBodyInnerV1::EditResponse { .. },
            InteractionEffectPlannedTargetV1::EditResponse { .. },
        ) => {}
        _ => return Err(InteractionEffectDispatchBuildErrorV1::BodyBinding),
    }
    Ok(())
}

#[cfg(test)]
fn registration_dependency_count_v1(
    roles: &[InteractionEffectDispatchResourceDependencyV1],
    channels: &[InteractionEffectDispatchResourceDependencyV1],
    messages: &[InteractionEffectDispatchResourceDependencyV1],
) -> usize {
    roles
        .iter()
        .chain(channels)
        .chain(messages)
        .map(|resource| resource.dependency.action_index())
        .collect::<BTreeSet<_>>()
        .len()
}

#[cfg(test)]
fn encode_body_v1(body: &InteractionEffectDispatchBodyV1) -> Zeroizing<Vec<u8>> {
    let mut frame = CanonicalFrameV1::new(DISPATCH_BODY_DOMAIN_V1);
    frame.text(3, body.kind().code());
    match &body.inner {
        InteractionEffectDispatchBodyInnerV1::CreateRole { output_alias, name }
        | InteractionEffectDispatchBodyInnerV1::CreateChannel { output_alias, name } => {
            frame.text(10, output_alias);
            frame.text(11, name);
        }
        InteractionEffectDispatchBodyInnerV1::GrantRole
        | InteractionEffectDispatchBodyInnerV1::UpsertOverwrite => {}
        InteractionEffectDispatchBodyInnerV1::PostPanel {
            output_alias,
            content,
            buttons,
        } => {
            frame.text(10, output_alias);
            frame.text(11, content);
            for button in buttons {
                let mut nested = CanonicalFrameV1::new(b"button.v1\0");
                nested.u16(3, u16::from(button.index));
                nested.text(4, &button.label);
                nested.text(5, &button.custom_id);
                frame.nested(12, nested.finish());
            }
        }
        InteractionEffectDispatchBodyInnerV1::RegisterInstance {
            output_alias,
            instance_id,
            registration,
            roles,
            channels,
            messages,
        } => {
            frame.text(10, output_alias);
            frame.text(11, instance_id.as_str());
            frame.nested(12, encode_registration_v1(registration));
            encode_dependency_resources_v1(&mut frame, 13, roles);
            encode_dependency_resources_v1(&mut frame, 14, channels);
            encode_dependency_resources_v1(&mut frame, 15, messages);
        }
        InteractionEffectDispatchBodyInnerV1::TeardownInstance { manifest } => {
            frame.text(10, manifest.instance_id.as_str());
            frame.nested(11, encode_registration_v1(&manifest.registration));
            for resource in &manifest.roles {
                let mut nested = CanonicalFrameV1::new(b"role.v1\0");
                nested.text(3, &resource.alias);
                nested.u64(4, resource.role_id.get());
                frame.nested(12, nested.finish());
            }
            for resource in &manifest.channels {
                let mut nested = CanonicalFrameV1::new(b"channel.v1\0");
                nested.text(3, &resource.alias);
                nested.u64(4, resource.channel_id.get());
                frame.nested(13, nested.finish());
            }
            for resource in &manifest.messages {
                let mut nested = CanonicalFrameV1::new(b"message.v1\0");
                nested.text(3, &resource.alias);
                nested.u64(4, resource.channel_id.get());
                nested.u64(5, resource.message_id.get());
                frame.nested(14, nested.finish());
            }
        }
        InteractionEffectDispatchBodyInnerV1::EditResponse { content } => {
            frame.text(10, content);
        }
    }
    frame.finish()
}

#[cfg(test)]
fn encode_registration_v1(
    registration: &InteractionEffectDispatchRegistrationIdentityV1,
) -> Zeroizing<Vec<u8>> {
    let mut frame = CanonicalFrameV1::new(b"registration.v1\0");
    frame.text(3, &registration.ruleset_key);
    frame.u32(4, registration.ruleset_version.get());
    frame.text(5, &registration.kind.0);
    frame.u64(6, registration.created_by.get());
    frame.finish()
}

#[cfg(test)]
fn encode_dependency_resources_v1(
    frame: &mut CanonicalFrameV1,
    tag: u8,
    resources: &[InteractionEffectDispatchResourceDependencyV1],
) {
    for resource in resources {
        let mut nested = CanonicalFrameV1::new(b"dependency_resource.v1\0");
        nested.text(3, &resource.alias);
        nested.u16(4, resource.dependency.action_index().get());
        nested.text(5, resource.dependency.producer_identity_digest().as_str());
        nested.text(6, output_class_code_v1(resource.dependency.output_class()));
        frame.nested(tag, nested.finish());
    }
}

#[cfg(test)]
fn encode_action_v1(
    source: &InteractionEffectDispatchSourceIdentityV1,
    effect_cursor: InteractionEffectActionIndexV1,
    journal_entry: &InteractionEffectJournalPlanEntryV1,
    body_canonical: &[u8],
) -> Zeroizing<Vec<u8>> {
    let mut frame = CanonicalFrameV1::new(DISPATCH_ACTION_DOMAIN_V1);
    frame.text(3, &source.node_id);
    frame.u16(4, source.source_ordinal);
    frame.u16(5, effect_cursor.get());
    frame.text(6, journal_entry.definition().action().kind().code());
    frame.text(
        7,
        journal_entry
            .definition()
            .planned_identity_digest()
            .as_str(),
    );
    frame.text(8, journal_entry.expected_postimage_digest().as_str());
    frame.nested(9, body_canonical);
    frame.finish()
}

#[cfg(test)]
fn encode_prepared_v1(
    certificate: &InteractionActionPreflightCertificateV1,
    journal_plan: &InteractionEffectJournalPlanV1,
    actions: &[InteractionEffectDispatchActionV1],
) -> Zeroizing<Vec<u8>> {
    let mut frame = CanonicalFrameV1::new(PREPARED_DISPATCH_DOMAIN_V1);
    frame.text(
        2,
        "deferred_ephemeral_success_and_atomic_journal_plan_with_durable_failure_tail",
    );
    frame.u64(3, certificate.receipt_identity().application_id().get());
    frame.u64(4, certificate.receipt_identity().interaction_id().get());
    frame.text(5, certificate.action_plan_digest().as_str());
    frame.text(6, certificate.preflight_plan_digest().as_str());
    frame.text(7, certificate.snapshot_digest().as_str());
    frame.text(8, certificate.digest().as_str());
    frame.u16(9, u16::try_from(actions.len()).unwrap_or(u16::MAX));
    for (action, journal_entry) in actions.iter().zip(journal_plan.entries()) {
        let mut nested = CanonicalFrameV1::new(b"prepared_action.v1\0");
        nested.u16(3, action.effect_cursor.get());
        nested.text(4, &action.source.node_id);
        nested.u16(5, action.source.source_ordinal);
        nested.text(
            6,
            journal_entry
                .definition()
                .planned_identity_digest()
                .as_str(),
        );
        nested.text(7, journal_entry.expected_postimage_digest().as_str());
        nested.text(8, action.body_digest.as_str());
        nested.text(9, action.action_digest.as_str());
        frame.nested(10, nested.finish());
    }
    frame.finish()
}

#[cfg(test)]
fn output_class_code_v1(class: InteractionEffectOutputClassV1) -> &'static str {
    match class {
        InteractionEffectOutputClassV1::CreatedRole => "created_role",
        InteractionEffectOutputClassV1::CreatedChannel => "created_channel",
        InteractionEffectOutputClassV1::RoleMembership => "role_membership",
        InteractionEffectOutputClassV1::PermissionOverwrite => "permission_overwrite",
        InteractionEffectOutputClassV1::PostedMessage => "posted_message",
        InteractionEffectOutputClassV1::InstanceState => "instance_state",
        InteractionEffectOutputClassV1::OriginalResponse => "original_response",
    }
}

#[cfg(test)]
struct CanonicalFrameV1 {
    bytes: Vec<u8>,
}

#[cfg(test)]
impl CanonicalFrameV1 {
    fn new(domain: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(domain.len() + 64);
        bytes.extend_from_slice(domain);
        bytes.extend_from_slice(&CANONICAL_VERSION_V1.to_be_bytes());
        Self { bytes }
    }

    fn field(&mut self, tag: u8, value: &[u8]) {
        self.bytes.push(tag);
        self.bytes
            .extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        self.bytes.extend_from_slice(value);
    }

    fn text(&mut self, tag: u8, value: &str) {
        self.field(tag, value.as_bytes());
    }

    fn u16(&mut self, tag: u8, value: u16) {
        self.field(tag, &value.to_be_bytes());
    }

    fn u32(&mut self, tag: u8, value: u32) {
        self.field(tag, &value.to_be_bytes());
    }

    fn u64(&mut self, tag: u8, value: u64) {
        self.field(tag, &value.to_be_bytes());
    }

    fn nested(&mut self, tag: u8, value: impl AsRef<[u8]>) {
        self.field(tag, value.as_ref());
    }

    fn finish(mut self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(std::mem::take(&mut self.bytes))
    }
}

#[cfg(test)]
impl Drop for CanonicalFrameV1 {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[cfg(test)]
fn lower_hex_v1(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_not_impl_any;

    use super::*;
    use crate::test_support::{certificate_v1, create_role_entry_v1, edit_response_entry_v1};

    assert_not_impl_any!(PreparedInteractionEffectDispatchV1: Clone);

    #[test]
    fn stateful_integration_is_explicitly_unavailable() {
        const { assert!(!STATEFUL_INTERACTION_EFFECT_DISPATCH_INTEGRATED_V1) };
        assert_eq!(
            require_stateful_interaction_effect_dispatch_integration_v1(),
            Err(StatefulInteractionEffectDispatchUnavailableV1)
        );
    }

    #[test]
    fn source_identity_is_bounded_and_canonical() {
        assert!(
            InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                "node/one:effect-0".to_string(),
                0,
            )
            .is_ok()
        );
        assert_eq!(
            InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(String::new(), 0),
            Err(InteractionEffectDispatchBuildErrorV1::SourceIdentity)
        );
        assert_eq!(
            InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                "x".repeat(MAX_SOURCE_NODE_ID_BYTES_V1 + 1),
                0,
            ),
            Err(InteractionEffectDispatchBuildErrorV1::SourceIdentity)
        );
    }

    #[test]
    fn exact_body_digest_changes_with_private_content() {
        let first = InteractionEffectDispatchBodyV1::edit_response_v1("done".to_string()).unwrap();
        let second =
            InteractionEffectDispatchBodyV1::edit_response_v1("done!".to_string()).unwrap();
        let first =
            InteractionEffectDispatchBodyDigestV1::from_canonical_v1(&encode_body_v1(&first));
        let second =
            InteractionEffectDispatchBodyDigestV1::from_canonical_v1(&encode_body_v1(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn public_debug_is_redacted() {
        let body = InteractionEffectDispatchBodyV1::create_role_v1(
            "moderator".to_string(),
            "Secret Role".to_string(),
        )
        .unwrap();
        let rendered = format!("{body:?}");
        assert!(!rendered.contains("Secret Role"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn prepared_digest_is_deterministic_and_binds_source_and_exact_body() {
        fn prepare(node_id: &str, content: &str) -> PreparedInteractionEffectDispatchV1 {
            let certificate = certificate_v1();
            let journal_plan = InteractionEffectJournalPlanV1::bind(
                &certificate,
                vec![edit_response_entry_v1(&certificate, 0)],
            )
            .unwrap();
            PreparedInteractionEffectDispatchV1::from_test_scaffold_v1(
                certificate,
                journal_plan,
                vec![(
                    InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                        node_id.to_string(),
                        7,
                    )
                    .unwrap(),
                    InteractionEffectDispatchBodyV1::edit_response_v1(content.to_string()).unwrap(),
                )],
            )
            .unwrap()
        }

        let first = prepare("response-node", "done");
        let replay = prepare("response-node", "done");
        let source_drift = prepare("response-node-2", "done");
        let body_drift = prepare("response-node", "done!");
        assert_eq!(first.digest(), replay.digest());
        assert_ne!(first.digest(), source_drift.digest());
        assert_ne!(first.digest(), body_drift.digest());
        assert_eq!(
            first.receipt_requirement(),
            InteractionEffectDispatchReceiptRequirementV1::DeferredEphemeralSuccessAndAtomicJournalPlan
        );
    }

    #[test]
    fn prepared_dispatch_rejects_source_reordering_and_body_kind_drift() {
        let certificate = certificate_v1();
        let journal_plan = InteractionEffectJournalPlanV1::bind(
            &certificate,
            vec![
                create_role_entry_v1(&certificate, 0),
                edit_response_entry_v1(&certificate, 1),
            ],
        )
        .unwrap();
        let result = PreparedInteractionEffectDispatchV1::from_test_scaffold_v1(
            certificate,
            journal_plan,
            vec![
                (
                    InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                        "create-role".to_string(),
                        2,
                    )
                    .unwrap(),
                    InteractionEffectDispatchBodyV1::create_role_v1(
                        "moderator".to_string(),
                        "Moderator".to_string(),
                    )
                    .unwrap(),
                ),
                (
                    InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                        "response".to_string(),
                        1,
                    )
                    .unwrap(),
                    InteractionEffectDispatchBodyV1::edit_response_v1("done".to_string()).unwrap(),
                ),
            ],
        );
        assert_eq!(
            result,
            Err(InteractionEffectDispatchBuildErrorV1::SourceOrder)
        );

        let certificate = certificate_v1();
        let journal_plan = InteractionEffectJournalPlanV1::bind(
            &certificate,
            vec![edit_response_entry_v1(&certificate, 0)],
        )
        .unwrap();
        let result = PreparedInteractionEffectDispatchV1::from_test_scaffold_v1(
            certificate,
            journal_plan,
            vec![(
                InteractionEffectDispatchSourceIdentityV1::from_test_scaffold_v1(
                    "response".to_string(),
                    1,
                )
                .unwrap(),
                InteractionEffectDispatchBodyV1::create_role_v1(
                    "moderator".to_string(),
                    "Moderator".to_string(),
                )
                .unwrap(),
            )],
        );
        assert_eq!(
            result,
            Err(InteractionEffectDispatchBuildErrorV1::BodyBinding)
        );
    }
}
