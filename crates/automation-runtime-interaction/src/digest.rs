use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use automation_ruleset::RuleSetContentHash;
use automation_runtime_convergence::RuntimeProcessIdentityV1;
use discord_model::{ChannelId, GuildId, UserId};
use sha2::{Digest, Sha256};

use crate::{
    InteractionExecutionRouteV1, InteractionProductScopeV1, InteractionReceiptClaimRootV1,
    InteractionReceiptIdentityV1, InteractionRouteBindingV1,
};

const REQUEST_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.request.v1\0";
const CLAIM_ROOT_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.claim_root.v1\0";
const ACTION_PLAN_DIGEST_DOMAIN_V1: &[u8] = b"starring.runtime.interaction.action_plan.v1\0";
const PREFLIGHT_CERTIFICATE_DIGEST_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.preflight_certificate.v1\0";
const MAX_CUSTOM_ID_BYTES: usize = 100;
const MAX_LOCALE_BYTES: usize = 64;
const MAX_MODAL_INPUTS: usize = 5;
const MAX_MODAL_INPUT_VALUE_BYTES: usize = 4_000;
const MAX_MODAL_PAYLOAD_BYTES: usize = 20_000;
const MAX_ACTION_KIND_BYTES: usize = 64;
const MAX_ACTIONS: usize = 256;
const MAX_ACTION_PAYLOAD_BYTES: usize = 65_536;
const MAX_ACTION_PLAN_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionDigestErrorV1 {
    #[error("interaction digest must contain exactly 64 characters")]
    Length,
    #[error("interaction digest must be lowercase hexadecimal")]
    LowerHex,
}

macro_rules! define_digest {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, InteractionDigestErrorV1> {
                let value = value.into();
                validate_digest(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

define_digest!(InteractionRequestDigestV1);
define_digest!(InteractionReceiptClaimRootDigestV1);
define_digest!(InteractionActionPlanDigestV1);
define_digest!(InteractionPreflightPlanDigestV1);
define_digest!(InteractionPreflightSnapshotDigestV1);
define_digest!(InteractionPreflightCertificateDigestV1);
define_digest!(InteractionInstanceManifestDigestV1);
define_digest!(InteractionRouteAttestationDigestV1);
define_digest!(InteractionTokenAuthenticatedDataDigestV1);

impl InteractionRequestDigestV1 {
    fn from_sha256(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

impl InteractionReceiptClaimRootDigestV1 {
    fn from_sha256(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

/// Commits the complete authoritative receipt claim using the same canonical encoder consumed by
/// the action-plan and preflight identities. This includes product scope, active process identity,
/// static or instance execution-route pins, serving and gateway fences, receipt identity, and the
/// verified request digest.
pub fn build_interaction_receipt_claim_root_digest_v1(
    claim_root: &InteractionReceiptClaimRootV1,
) -> InteractionReceiptClaimRootDigestV1 {
    let mut bytes = Vec::with_capacity(2_048);
    append_frame(&mut bytes, CLAIM_ROOT_DIGEST_DOMAIN_V1);
    append_claim_root_v1(&mut bytes, claim_root);
    InteractionReceiptClaimRootDigestV1::from_sha256(&bytes)
}

impl InteractionActionPlanDigestV1 {
    fn from_sha256(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

impl InteractionPreflightPlanDigestV1 {
    pub fn from_canonical_bytes(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

impl InteractionPreflightSnapshotDigestV1 {
    pub fn from_canonical_bytes(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

impl InteractionPreflightCertificateDigestV1 {
    fn from_sha256(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

pub struct InteractionPreflightCertificateDigestInputV1<'a> {
    pub claim_root: &'a InteractionReceiptClaimRootV1,
    pub action_plan_digest: &'a InteractionActionPlanDigestV1,
    pub preflight_plan_digest: &'a InteractionPreflightPlanDigestV1,
    pub snapshot_digest: &'a InteractionPreflightSnapshotDigestV1,
}

pub fn build_interaction_preflight_certificate_digest_v1(
    input: InteractionPreflightCertificateDigestInputV1<'_>,
) -> InteractionPreflightCertificateDigestV1 {
    let mut bytes = Vec::with_capacity(2_048);
    append_frame(&mut bytes, PREFLIGHT_CERTIFICATE_DIGEST_DOMAIN_V1);
    append_claim_root_v1(&mut bytes, input.claim_root);
    append_field(
        &mut bytes,
        b"action_plan_digest",
        input.action_plan_digest.as_str().as_bytes(),
    );
    append_field(
        &mut bytes,
        b"preflight_plan_digest",
        input.preflight_plan_digest.as_str().as_bytes(),
    );
    append_field(
        &mut bytes,
        b"snapshot_digest",
        input.snapshot_digest.as_str().as_bytes(),
    );
    InteractionPreflightCertificateDigestV1::from_sha256(&bytes)
}

impl InteractionTokenAuthenticatedDataDigestV1 {
    pub(crate) fn from_sha256(bytes: &[u8]) -> Self {
        Self(lower_hex(Sha256::digest(bytes).as_slice()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionRequestPayloadV1<'a> {
    Button {
        custom_id: &'a str,
    },
    ModalSubmit {
        custom_id: &'a str,
        inputs: &'a BTreeMap<String, String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionRequestDigestInputV1<'a> {
    pub receipt_identity: InteractionReceiptIdentityV1,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub actor_id: UserId,
    pub locale: Option<&'a str>,
    pub payload: InteractionRequestPayloadV1<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionRequestDigestErrorV1 {
    #[error("interaction request Discord identity must be non-zero")]
    Identity,
    #[error("interaction request custom identifier is invalid")]
    CustomId,
    #[error("interaction request locale is invalid")]
    Locale,
    #[error("interaction request modal input count is invalid")]
    ModalInputCount,
    #[error("interaction request modal input is invalid")]
    ModalInput,
    #[error("interaction request modal payload exceeds the supported size")]
    ModalPayload,
}

pub fn build_interaction_request_digest_v1(
    input: InteractionRequestDigestInputV1<'_>,
) -> Result<InteractionRequestDigestV1, InteractionRequestDigestErrorV1> {
    if input.guild_id.0 == 0 || input.channel_id.0 == 0 || input.actor_id.0 == 0 {
        return Err(InteractionRequestDigestErrorV1::Identity);
    }
    validate_locale(input.locale)?;
    let mut bytes = Vec::with_capacity(1_024);
    append_frame(&mut bytes, REQUEST_DIGEST_DOMAIN_V1);
    append_receipt_identity_v1(&mut bytes, input.receipt_identity);
    append_field(&mut bytes, b"guild_id", &input.guild_id.0.to_be_bytes());
    append_field(&mut bytes, b"channel_id", &input.channel_id.0.to_be_bytes());
    append_field(&mut bytes, b"actor_id", &input.actor_id.0.to_be_bytes());
    append_optional_field(&mut bytes, b"locale", input.locale.map(str::as_bytes));
    match input.payload {
        InteractionRequestPayloadV1::Button { custom_id } => {
            validate_custom_id(custom_id)?;
            append_field(&mut bytes, b"kind", b"button");
            append_field(&mut bytes, b"custom_id", custom_id.as_bytes());
        }
        InteractionRequestPayloadV1::ModalSubmit { custom_id, inputs } => {
            validate_custom_id(custom_id)?;
            if inputs.is_empty() || inputs.len() > MAX_MODAL_INPUTS {
                return Err(InteractionRequestDigestErrorV1::ModalInputCount);
            }
            let mut payload_bytes = 0_usize;
            append_field(&mut bytes, b"kind", b"modal_submit");
            append_field(&mut bytes, b"custom_id", custom_id.as_bytes());
            append_field(
                &mut bytes,
                b"input_count",
                &(inputs.len() as u64).to_be_bytes(),
            );
            for (input_id, value) in inputs {
                if input_id.is_empty()
                    || input_id.len() > MAX_CUSTOM_ID_BYTES
                    || value.len() > MAX_MODAL_INPUT_VALUE_BYTES
                    || input_id.bytes().any(|byte| byte == 0)
                {
                    return Err(InteractionRequestDigestErrorV1::ModalInput);
                }
                payload_bytes = payload_bytes
                    .checked_add(input_id.len())
                    .and_then(|total| total.checked_add(value.len()))
                    .ok_or(InteractionRequestDigestErrorV1::ModalPayload)?;
                if payload_bytes > MAX_MODAL_PAYLOAD_BYTES {
                    return Err(InteractionRequestDigestErrorV1::ModalPayload);
                }
                append_field(&mut bytes, b"input_id", input_id.as_bytes());
                append_field(&mut bytes, b"input_value", value.as_bytes());
            }
        }
    }
    Ok(InteractionRequestDigestV1::from_sha256(&bytes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionActionPlanDigestBuilderErrorV1 {
    #[error("interaction action kind is invalid")]
    InvalidActionKind,
    #[error("interaction action plan contains too many actions")]
    TooManyActions,
    #[error("interaction action payload exceeds the supported size")]
    ActionPayloadTooLarge,
    #[error("interaction action plan exceeds the supported size")]
    ActionPlanTooLarge,
    #[error("interaction action plan must contain at least one action")]
    EmptyActionPlan,
}

#[derive(Clone, Debug)]
pub struct InteractionActionPlanDigestBuilderV1 {
    bytes: Vec<u8>,
    action_count: usize,
    payload_bytes: usize,
}

impl InteractionActionPlanDigestBuilderV1 {
    pub fn new(
        route: &InteractionRouteBindingV1,
        request_digest: &InteractionRequestDigestV1,
        defer_ephemeral: bool,
    ) -> Self {
        let mut bytes = Vec::with_capacity(2_048);
        append_frame(&mut bytes, ACTION_PLAN_DIGEST_DOMAIN_V1);
        append_route_binding_v1(&mut bytes, route);
        append_field(
            &mut bytes,
            b"request_digest",
            request_digest.as_str().as_bytes(),
        );
        append_field(&mut bytes, b"defer_ephemeral", &[u8::from(defer_ephemeral)]);
        Self {
            bytes,
            action_count: 0,
            payload_bytes: 0,
        }
    }

    pub fn push_action(
        &mut self,
        kind: &str,
        canonical_payload: &[u8],
    ) -> Result<(), InteractionActionPlanDigestBuilderErrorV1> {
        if kind.is_empty()
            || kind.len() > MAX_ACTION_KIND_BYTES
            || !kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(InteractionActionPlanDigestBuilderErrorV1::InvalidActionKind);
        }
        if self.action_count >= MAX_ACTIONS {
            return Err(InteractionActionPlanDigestBuilderErrorV1::TooManyActions);
        }
        if canonical_payload.len() > MAX_ACTION_PAYLOAD_BYTES {
            return Err(InteractionActionPlanDigestBuilderErrorV1::ActionPayloadTooLarge);
        }
        let next_payload_bytes = self
            .payload_bytes
            .checked_add(kind.len())
            .and_then(|total| total.checked_add(canonical_payload.len()))
            .ok_or(InteractionActionPlanDigestBuilderErrorV1::ActionPlanTooLarge)?;
        if next_payload_bytes > MAX_ACTION_PLAN_BYTES {
            return Err(InteractionActionPlanDigestBuilderErrorV1::ActionPlanTooLarge);
        }
        append_field(
            &mut self.bytes,
            b"action_index",
            &(self.action_count as u64).to_be_bytes(),
        );
        append_field(&mut self.bytes, b"action_kind", kind.as_bytes());
        append_field(&mut self.bytes, b"action_payload", canonical_payload);
        self.action_count += 1;
        self.payload_bytes = next_payload_bytes;
        Ok(())
    }

    pub fn finish(
        mut self,
    ) -> Result<InteractionActionPlanDigestV1, InteractionActionPlanDigestBuilderErrorV1> {
        if self.action_count == 0 {
            return Err(InteractionActionPlanDigestBuilderErrorV1::EmptyActionPlan);
        }
        append_field(
            &mut self.bytes,
            b"action_count",
            &(self.action_count as u64).to_be_bytes(),
        );
        Ok(InteractionActionPlanDigestV1::from_sha256(&self.bytes))
    }
}

pub(crate) fn append_receipt_identity_v1(
    bytes: &mut Vec<u8>,
    identity: InteractionReceiptIdentityV1,
) {
    append_field(
        bytes,
        b"application_id",
        &identity.application_id().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"interaction_id",
        &identity.interaction_id().get().to_be_bytes(),
    );
}

pub(crate) fn append_route_binding_v1(bytes: &mut Vec<u8>, route: &InteractionRouteBindingV1) {
    append_product_scope_v1(bytes, route.scope());
    append_process_identity_v1(bytes, route.process_identity());
    let serving = route.serving_identity();
    append_field(
        bytes,
        b"route_attestation_digest",
        serving.attestation_digest().as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"route_lease_epoch",
        &serving.lease_epoch().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"route_lease_revision",
        &serving.lease_revision().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"gateway_shard_identity",
        serving.gateway_shard_identity().as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"gateway_owner_lease_epoch",
        &serving.gateway_owner_lease_epoch().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"gateway_owner_revision",
        &serving.gateway_owner_revision().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"runtime_build_revision",
        serving.runtime_build_revision().as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"route_fencing_token",
        &serving.route_fencing_token().get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"route_incarnation",
        &serving.route_incarnation().get().to_be_bytes(),
    );
    append_execution_route_v1(bytes, route.execution_route());
}

pub(crate) fn append_claim_root_v1(bytes: &mut Vec<u8>, root: &InteractionReceiptClaimRootV1) {
    append_receipt_identity_v1(bytes, root.identity());
    append_route_binding_v1(bytes, root.route());
    append_field(
        bytes,
        b"request_digest",
        root.request_digest().as_str().as_bytes(),
    );
}

fn append_product_scope_v1(bytes: &mut Vec<u8>, scope: &InteractionProductScopeV1) {
    append_field(bytes, b"tenant_id", scope.tenant_id().as_str().as_bytes());
    append_field(
        bytes,
        b"installation_id",
        scope.installation_id().as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"deployment_id",
        scope.deployment_id().as_str().as_bytes(),
    );
}

fn append_process_identity_v1(bytes: &mut Vec<u8>, process: &RuntimeProcessIdentityV1) {
    append_field(
        bytes,
        b"target_guild_id",
        &process.target.guild_id.0.to_be_bytes(),
    );
    append_field(
        bytes,
        b"ruleset_key",
        process.target.ruleset_key.as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"ruleset_version",
        &process.target.version.get().to_be_bytes(),
    );
    append_ruleset_hash(bytes, b"ruleset_content_hash", process.target.content_hash);
    append_field(
        bytes,
        b"binding_revision",
        &process.target.binding_revision.get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"binding_fingerprint",
        process.target.binding_fingerprint.as_str().as_bytes(),
    );
    append_field(
        bytes,
        b"runtime_generation",
        &process.runtime_generation.get().to_be_bytes(),
    );
    append_field(
        bytes,
        b"process_instance_id",
        process.process_instance_id.as_str().as_bytes(),
    );
}

fn append_execution_route_v1(bytes: &mut Vec<u8>, route: &InteractionExecutionRouteV1) {
    match route {
        InteractionExecutionRouteV1::Static {
            ruleset_version,
            ruleset_content_hash,
        } => {
            append_field(bytes, b"execution_route_kind", b"static");
            append_field(
                bytes,
                b"execution_ruleset_version",
                &ruleset_version.get().to_be_bytes(),
            );
            append_ruleset_hash(
                bytes,
                b"execution_ruleset_content_hash",
                *ruleset_content_hash,
            );
        }
        InteractionExecutionRouteV1::Instance {
            instance_id,
            pinned_ruleset_version,
            pinned_ruleset_content_hash,
            resource_manifest_digest,
        } => {
            append_field(bytes, b"execution_route_kind", b"instance");
            append_field(bytes, b"instance_id", instance_id.as_str().as_bytes());
            append_field(
                bytes,
                b"pinned_ruleset_version",
                &pinned_ruleset_version.get().to_be_bytes(),
            );
            append_ruleset_hash(
                bytes,
                b"pinned_ruleset_content_hash",
                *pinned_ruleset_content_hash,
            );
            append_field(
                bytes,
                b"resource_manifest_digest",
                resource_manifest_digest.as_str().as_bytes(),
            );
        }
    }
}

pub(crate) fn append_field(bytes: &mut Vec<u8>, name: &[u8], value: &[u8]) {
    append_frame(bytes, name);
    append_frame(bytes, value);
}

pub(crate) fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing hexadecimal to String cannot fail");
    }
    output
}

fn validate_digest(value: &str) -> Result<(), InteractionDigestErrorV1> {
    if value.len() != 64 {
        return Err(InteractionDigestErrorV1::Length);
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(InteractionDigestErrorV1::LowerHex);
    }
    Ok(())
}

fn validate_custom_id(custom_id: &str) -> Result<(), InteractionRequestDigestErrorV1> {
    if custom_id.is_empty()
        || custom_id.len() > MAX_CUSTOM_ID_BYTES
        || custom_id.bytes().any(|byte| byte == 0)
    {
        return Err(InteractionRequestDigestErrorV1::CustomId);
    }
    Ok(())
}

fn validate_locale(locale: Option<&str>) -> Result<(), InteractionRequestDigestErrorV1> {
    if locale.is_some_and(|value| {
        value.is_empty() || value.len() > MAX_LOCALE_BYTES || value.chars().any(char::is_control)
    }) {
        return Err(InteractionRequestDigestErrorV1::Locale);
    }
    Ok(())
}

fn append_optional_field(bytes: &mut Vec<u8>, name: &[u8], value: Option<&[u8]>) {
    match value {
        Some(value) => {
            append_field(bytes, name, b"some");
            append_field(bytes, b"optional_value", value);
        }
        None => append_field(bytes, name, b"none"),
    }
}

fn append_ruleset_hash(bytes: &mut Vec<u8>, name: &[u8], hash: RuleSetContentHash) {
    append_field(bytes, name, hash.to_hex().as_bytes());
}

fn append_frame(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use automation_ruleset::RuleSetVersionId;
    use discord_model::{ChannelId, GuildId, UserId};

    use super::*;
    use crate::test_support::{
        instance_route, static_route, static_route_with_build_revision,
        static_route_with_gateway_owner,
    };

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            crate::DiscordApplicationIdV1::new(10).unwrap(),
            crate::DiscordInteractionIdV1::new(20).unwrap(),
        )
    }

    fn button_request(custom_id: &str) -> InteractionRequestDigestV1 {
        build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
            receipt_identity: receipt(),
            guild_id: GuildId(30),
            channel_id: ChannelId(40),
            actor_id: UserId(50),
            locale: Some("ko"),
            payload: InteractionRequestPayloadV1::Button { custom_id },
        })
        .unwrap()
    }

    #[test]
    fn request_digest_is_canonical_and_sensitive_to_semantic_input() {
        let first = button_request("join");
        let second = button_request("join");
        let changed = button_request("leave");
        assert_eq!(first, second);
        assert_ne!(first, changed);
        assert_eq!(first.as_str().len(), 64);
        assert!(first
            .as_str()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn modal_digest_uses_sorted_input_identity_and_rejects_invalid_payloads() {
        let mut inputs = BTreeMap::new();
        inputs.insert("name".to_string(), "study".to_string());
        inputs.insert("topic".to_string(), "rust".to_string());
        let digest = build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
            receipt_identity: receipt(),
            guild_id: GuildId(30),
            channel_id: ChannelId(40),
            actor_id: UserId(50),
            locale: None,
            payload: InteractionRequestPayloadV1::ModalSubmit {
                custom_id: "submit_room",
                inputs: &inputs,
            },
        })
        .unwrap();
        assert_eq!(digest.as_str().len(), 64);

        let empty = BTreeMap::new();
        assert_eq!(
            build_interaction_request_digest_v1(InteractionRequestDigestInputV1 {
                receipt_identity: receipt(),
                guild_id: GuildId(30),
                channel_id: ChannelId(40),
                actor_id: UserId(50),
                locale: None,
                payload: InteractionRequestPayloadV1::ModalSubmit {
                    custom_id: "submit_room",
                    inputs: &empty,
                },
            }),
            Err(InteractionRequestDigestErrorV1::ModalInputCount)
        );
    }

    #[test]
    fn action_plan_digest_binds_route_order_and_deferral_policy() {
        let route = static_route(1);
        let request = button_request("join");
        let mut first = InteractionActionPlanDigestBuilderV1::new(&route, &request, true);
        first.push_action("grant_role", b"role=member").unwrap();
        first
            .push_action("respond_ephemeral", b"message=joined")
            .unwrap();
        let first = first.finish().unwrap();

        let mut reordered = InteractionActionPlanDigestBuilderV1::new(&route, &request, true);
        reordered
            .push_action("respond_ephemeral", b"message=joined")
            .unwrap();
        reordered.push_action("grant_role", b"role=member").unwrap();
        let reordered = reordered.finish().unwrap();

        let changed_route = static_route(2);
        let mut changed =
            InteractionActionPlanDigestBuilderV1::new(&changed_route, &request, false);
        changed.push_action("grant_role", b"role=member").unwrap();
        changed
            .push_action("respond_ephemeral", b"message=joined")
            .unwrap();

        assert_ne!(first, reordered);
        assert_ne!(first, changed.finish().unwrap());
    }

    #[test]
    fn claim_root_digest_binds_static_and_instance_execution_routes() {
        let request = button_request("join");
        let bind = |route: crate::InteractionRouteBindingV1| {
            crate::InteractionReceiptClaimCandidateV1::new(
                receipt(),
                crate::InteractionExpectedRouteV1::from_authoritative(&route),
                request.clone(),
            )
            .bind_authoritative(route)
            .unwrap()
        };
        let static_claim = bind(static_route(1));
        let instance_claim = bind(instance_route(1));

        let first = build_interaction_receipt_claim_root_digest_v1(&static_claim);
        assert_eq!(
            first,
            build_interaction_receipt_claim_root_digest_v1(&static_claim)
        );
        assert_ne!(
            first,
            build_interaction_receipt_claim_root_digest_v1(&instance_claim)
        );
    }

    #[test]
    fn action_plan_digest_binds_database_derived_gateway_owner_fence() {
        let request = button_request("join");
        let mut first = InteractionActionPlanDigestBuilderV1::new(
            &static_route_with_gateway_owner(1, 1),
            &request,
            true,
        );
        first.push_action("grant_role", b"role=member").unwrap();
        let mut renewed = InteractionActionPlanDigestBuilderV1::new(
            &static_route_with_gateway_owner(1, 2),
            &request,
            true,
        );
        renewed.push_action("grant_role", b"role=member").unwrap();

        assert_ne!(first.finish().unwrap(), renewed.finish().unwrap());
    }

    #[test]
    fn action_plan_digest_binds_runtime_build_revision() {
        let request = button_request("join");
        let mut first = InteractionActionPlanDigestBuilderV1::new(
            &static_route_with_build_revision(1, 1),
            &request,
            true,
        );
        first.push_action("grant_role", b"role=member").unwrap();
        let mut changed = InteractionActionPlanDigestBuilderV1::new(
            &static_route_with_build_revision(1, 2),
            &request,
            true,
        );
        changed.push_action("grant_role", b"role=member").unwrap();

        assert_ne!(first.finish().unwrap(), changed.finish().unwrap());
    }

    #[test]
    fn preflight_certificate_is_deterministic_and_binds_every_authoritative_input() {
        let route = static_route(1);
        let request = button_request("join");
        let candidate = crate::InteractionReceiptClaimCandidateV1::new(
            receipt(),
            crate::InteractionExpectedRouteV1::from_authoritative(&route),
            request.clone(),
        );
        let claim = candidate.bind_authoritative(route).unwrap();
        let mut builder = InteractionActionPlanDigestBuilderV1::new(claim.route(), &request, false);
        builder.push_action("grant_role", b"role=member").unwrap();
        let action_plan = builder.finish().unwrap();
        let preflight_plan = InteractionPreflightPlanDigestV1::from_canonical_bytes(b"plan");
        let snapshot = InteractionPreflightSnapshotDigestV1::from_canonical_bytes(b"snapshot");
        let build = |claim: &InteractionReceiptClaimRootV1,
                     action_plan: &InteractionActionPlanDigestV1,
                     preflight_plan: &InteractionPreflightPlanDigestV1,
                     snapshot: &InteractionPreflightSnapshotDigestV1| {
            build_interaction_preflight_certificate_digest_v1(
                InteractionPreflightCertificateDigestInputV1 {
                    claim_root: claim,
                    action_plan_digest: action_plan,
                    preflight_plan_digest: preflight_plan,
                    snapshot_digest: snapshot,
                },
            )
        };
        let first = build(&claim, &action_plan, &preflight_plan, &snapshot);
        assert_eq!(
            first,
            build(&claim, &action_plan, &preflight_plan, &snapshot)
        );
        assert_ne!(
            first,
            build(
                &claim,
                &action_plan,
                &InteractionPreflightPlanDigestV1::from_canonical_bytes(b"changed"),
                &snapshot,
            )
        );
        assert_ne!(
            first,
            build(
                &claim,
                &action_plan,
                &preflight_plan,
                &InteractionPreflightSnapshotDigestV1::from_canonical_bytes(b"changed"),
            )
        );

        let changed_route = static_route(2);
        let changed_claim = crate::InteractionReceiptClaimCandidateV1::new(
            receipt(),
            crate::InteractionExpectedRouteV1::from_authoritative(&changed_route),
            request,
        )
        .bind_authoritative(changed_route)
        .unwrap();
        assert_ne!(
            first,
            build(&changed_claim, &action_plan, &preflight_plan, &snapshot)
        );
    }

    #[test]
    fn digest_parsing_and_action_limits_fail_closed() {
        assert_eq!(
            InteractionRequestDigestV1::parse("a"),
            Err(InteractionDigestErrorV1::Length)
        );
        assert_eq!(
            InteractionRequestDigestV1::parse("A".repeat(64)),
            Err(InteractionDigestErrorV1::LowerHex)
        );
        let route = static_route(1);
        let request = button_request("join");
        let builder = InteractionActionPlanDigestBuilderV1::new(&route, &request, false);
        assert_eq!(
            builder.finish(),
            Err(InteractionActionPlanDigestBuilderErrorV1::EmptyActionPlan)
        );
        let mut builder = InteractionActionPlanDigestBuilderV1::new(&route, &request, false);
        assert_eq!(
            builder.push_action("GrantRole", b"role=member"),
            Err(InteractionActionPlanDigestBuilderErrorV1::InvalidActionKind)
        );
        assert_eq!(RuleSetVersionId::FIRST.get(), 1);
    }
}
