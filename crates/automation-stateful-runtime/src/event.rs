use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use automation_runtime_interaction::{
    build_interaction_receipt_claim_root_digest_v1, decode_interaction_custom_id_v1,
    InteractionComponentKindV1, InteractionExecutionRouteV1, InteractionReceiptClaimRootDigestV1,
    InteractionReceiptIdentityV1, InteractionRequestDigestV1, ParsedInteractionCustomIdV1,
    VerifiedInteractionRequestKindV1, VerifiedInteractionRequestV1,
};
use automation_stateful_compiler::CompiledStatefulBundleV1;
use automation_stateful_spec::{
    normalize_stateful_event_inputs_v1, stateful_spec_digest_v1, StatefulEventNormalizationErrorV1,
    StatefulSpecDigestV1, StatefulSpecV1, TriggerV1,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::STATEFUL_EVALUATOR_REVISION_V1;

pub const EVENT_ENVELOPE_SCHEMA_VERSION_V1: u16 = 1;
pub const EVENT_ENVELOPE_KIND_V1: &str = "starring.stateful-event-envelope.v1";
const EVENT_ENVELOPE_DOMAIN_V1: &[u8] = b"starring.stateful_event_envelope.v1\0";

macro_rules! define_digest {
    ($name:ident, $error:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, $error> {
                let value = value.into();
                if valid_digest(&value) {
                    Ok(Self(value))
                } else {
                    Err($error::InvalidDigest)
                }
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

define_digest!(StatefulArtifactDigestV1, StatefulProgramIdentityErrorV1);
define_digest!(StateSchemaDigestV1, StatefulProgramIdentityErrorV1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyRuleSetIdentityV1 {
    key: String,
    version: u32,
    content_hash: String,
}

impl LegacyRuleSetIdentityV1 {
    pub fn new(
        key: impl Into<String>,
        version: u32,
        content_hash: impl Into<String>,
    ) -> Result<Self, StatefulProgramIdentityErrorV1> {
        let key = key.into();
        let content_hash = content_hash.into();
        if !valid_identifier(&key) || version == 0 || !valid_digest(&content_hash) {
            return Err(StatefulProgramIdentityErrorV1::InvalidLegacyRuleSet);
        }
        Ok(Self {
            key,
            version,
            content_hash,
        })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

/// Immutable identity of the validated stateful program and the separate filtered legacy
/// RuleSet artifact that is allowed to handle only its stateless workflows.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulProgramIdentityV1 {
    program_key: String,
    source_spec_digest: StatefulSpecDigestV1,
    bundle_digest: String,
    compilation_binding_digest: String,
    union_source_map_digest: String,
    stateful_artifact_digest: StatefulArtifactDigestV1,
    state_schema_digest: StateSchemaDigestV1,
    compiler_revision: u32,
    evaluator_revision: u32,
    filtered_legacy_ruleset: LegacyRuleSetIdentityV1,
}

impl StatefulProgramIdentityV1 {
    fn from_compiled_bundle(
        bundle: &CompiledStatefulBundleV1,
        publication: &StatefulBundlePublicationBindingV1,
    ) -> Result<Self, StatefulProgramIdentityErrorV1> {
        let source = bundle.stateful_artifact().source().digest();
        let bundle_digest = bundle.bundle_digest().to_hex();
        let binding_digest = bundle.binding_digest().to_hex();
        let source_map_digest = bundle.union_source_map_digest().to_hex();
        if publication.bundle_digest != bundle_digest
            || publication.compilation_binding_digest != binding_digest
            || publication.union_source_map_digest != source_map_digest
            || publication.source_spec_digest != source.to_hex()
            || publication.stateful_artifact_digest != bundle.stateful_artifact_digest().to_hex()
            || publication.state_schema_digest != bundle.state_schema_digest().to_hex()
            || publication.program_key != bundle.stateful_artifact().program_key()
            || publication.filtered_legacy_content_hash
                != bundle.filtered_legacy_target().content_hash.to_hex()
        {
            return Err(StatefulProgramIdentityErrorV1::PublicationMismatch);
        }
        Ok(Self {
            program_key: bundle.stateful_artifact().program_key().to_string(),
            source_spec_digest: source,
            bundle_digest,
            compilation_binding_digest: binding_digest,
            union_source_map_digest: source_map_digest,
            stateful_artifact_digest: StatefulArtifactDigestV1(
                bundle.stateful_artifact_digest().to_hex(),
            ),
            state_schema_digest: StateSchemaDigestV1(bundle.state_schema_digest().to_hex()),
            compiler_revision: automation_stateful_compiler::STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
            evaluator_revision: STATEFUL_EVALUATOR_REVISION_V1,
            filtered_legacy_ruleset: LegacyRuleSetIdentityV1 {
                key: publication.program_key.clone(),
                version: publication.execution_ruleset_version,
                content_hash: publication.filtered_legacy_content_hash.clone(),
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn from_validated_spec(
        spec: &StatefulSpecV1,
        stateful_artifact_digest: StatefulArtifactDigestV1,
        state_schema_digest: StateSchemaDigestV1,
        filtered_legacy_ruleset: LegacyRuleSetIdentityV1,
    ) -> Result<Self, StatefulProgramIdentityErrorV1> {
        if filtered_legacy_ruleset.key != spec.key {
            return Err(StatefulProgramIdentityErrorV1::ProgramKeyMismatch);
        }
        Ok(Self {
            program_key: spec.key.clone(),
            source_spec_digest: stateful_spec_digest_v1(spec)
                .map_err(|_| StatefulProgramIdentityErrorV1::InvalidSpec)?,
            bundle_digest: "0".repeat(64),
            compilation_binding_digest: "0".repeat(64),
            union_source_map_digest: "0".repeat(64),
            stateful_artifact_digest,
            state_schema_digest,
            compiler_revision: crate::STATEFUL_COMPILER_REVISION_V1,
            evaluator_revision: STATEFUL_EVALUATOR_REVISION_V1,
            filtered_legacy_ruleset,
        })
    }

    pub fn program_key(&self) -> &str {
        &self.program_key
    }

    pub fn source_spec_digest(&self) -> StatefulSpecDigestV1 {
        self.source_spec_digest
    }

    pub fn bundle_digest(&self) -> &str {
        &self.bundle_digest
    }

    pub fn compilation_binding_digest(&self) -> &str {
        &self.compilation_binding_digest
    }

    pub fn union_source_map_digest(&self) -> &str {
        &self.union_source_map_digest
    }

    pub fn stateful_artifact_digest(&self) -> &StatefulArtifactDigestV1 {
        &self.stateful_artifact_digest
    }

    pub fn state_schema_digest(&self) -> &StateSchemaDigestV1 {
        &self.state_schema_digest
    }

    pub fn compiler_revision(&self) -> u32 {
        self.compiler_revision
    }

    pub fn evaluator_revision(&self) -> u32 {
        self.evaluator_revision
    }

    pub fn filtered_legacy_ruleset(&self) -> &LegacyRuleSetIdentityV1 {
        &self.filtered_legacy_ruleset
    }
}

impl std::fmt::Debug for StatefulProgramIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StatefulProgramIdentityV1")
            .field("program_key", &self.program_key)
            .field("source_spec_digest", &self.source_spec_digest.to_hex())
            .field("bundle_digest", &self.bundle_digest)
            .field(
                "compilation_binding_digest",
                &self.compilation_binding_digest,
            )
            .field("union_source_map_digest", &self.union_source_map_digest)
            .field(
                "stateful_artifact_digest",
                &self.stateful_artifact_digest.as_str(),
            )
            .field("state_schema_digest", &self.state_schema_digest.as_str())
            .field("compiler_revision", &self.compiler_revision)
            .field("evaluator_revision", &self.evaluator_revision)
            .field("filtered_legacy_ruleset", &self.filtered_legacy_ruleset)
            .finish()
    }
}

/// Opaque, non-deployable R0 publication binding for one exact compiler bundle and one verified
/// request route. A future control-plane publication-row verifier must issue it; the verified
/// route alone is deliberately insufficient. R0 provides only a test-only stand-in constructor.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulBundlePublicationBindingV1 {
    bundle_digest: String,
    compilation_binding_digest: String,
    union_source_map_digest: String,
    source_spec_digest: String,
    stateful_artifact_digest: String,
    state_schema_digest: String,
    program_key: String,
    execution_ruleset_version: u32,
    filtered_legacy_content_hash: String,
    claim_root_digest: String,
}

impl StatefulBundlePublicationBindingV1 {
    /// Test-only stand-in for the future control-plane publication-row verifier. A verified
    /// interaction route alone cannot establish authority for a stateful bundle, even when its
    /// filtered legacy key/hash agree.
    #[cfg(test)]
    pub(crate) fn from_test_authority(
        bundle: &CompiledStatefulBundleV1,
        verified: &VerifiedInteractionRequestV1,
    ) -> Result<Self, EventEnvelopeErrorV1> {
        let authoritative = verified.claim_root().route();
        let target = &authoritative.process_identity().target;
        let bundle_target = bundle.filtered_legacy_target();
        if target.ruleset_key.as_str() != bundle_target.ruleset_key
            || bundle.stateful_artifact().program_key() != bundle_target.ruleset_key
        {
            return Err(EventEnvelopeErrorV1::RouteMismatch);
        }
        let (version, content_hash) = match authoritative.execution_route() {
            InteractionExecutionRouteV1::Static {
                ruleset_version,
                ruleset_content_hash,
            } => (ruleset_version.get(), ruleset_content_hash.to_hex()),
            InteractionExecutionRouteV1::Instance {
                pinned_ruleset_version,
                pinned_ruleset_content_hash,
                ..
            } => (
                pinned_ruleset_version.get(),
                pinned_ruleset_content_hash.to_hex(),
            ),
        };
        if content_hash != bundle_target.content_hash.to_hex() {
            return Err(EventEnvelopeErrorV1::RouteMismatch);
        }
        Ok(Self {
            bundle_digest: bundle.bundle_digest().to_hex(),
            compilation_binding_digest: bundle.binding_digest().to_hex(),
            union_source_map_digest: bundle.union_source_map_digest().to_hex(),
            source_spec_digest: bundle.stateful_artifact().source().digest().to_hex(),
            stateful_artifact_digest: bundle.stateful_artifact_digest().to_hex(),
            state_schema_digest: bundle.state_schema_digest().to_hex(),
            program_key: bundle_target.ruleset_key.clone(),
            execution_ruleset_version: version,
            filtered_legacy_content_hash: content_hash,
            claim_root_digest: build_interaction_receipt_claim_root_digest_v1(
                verified.claim_root(),
            )
            .as_str()
            .to_string(),
        })
    }

    pub fn claim_root_digest(&self) -> &str {
        &self.claim_root_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventEnvelopeScopeV1 {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    guild_id: u64,
    channel_id: u64,
    actor_user_id: u64,
    instance_id: Option<String>,
    instance_manifest_digest: Option<String>,
}

impl EventEnvelopeScopeV1 {
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    pub fn deployment_id(&self) -> &str {
        &self.deployment_id
    }

    pub fn guild_id(&self) -> u64 {
        self.guild_id
    }

    pub fn channel_id(&self) -> u64 {
        self.channel_id
    }

    pub fn actor_user_id(&self) -> u64 {
        self.actor_user_id
    }

    pub fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }

    pub fn instance_manifest_digest(&self) -> Option<&str> {
        self.instance_manifest_digest.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventEnvelopeRouteV1 {
    active_ruleset_key: String,
    active_ruleset_version: u32,
    active_ruleset_content_hash: String,
    execution_ruleset_version: u32,
    execution_ruleset_content_hash: String,
    binding_revision: u64,
    binding_fingerprint: String,
    runtime_generation: u64,
    process_instance_id: String,
    route_attestation_digest: String,
    serving_lease_epoch: u64,
    serving_lease_revision: u64,
    gateway_shard_identity: String,
    gateway_owner_lease_epoch: u64,
    gateway_owner_revision: u64,
    runtime_build_revision: String,
    route_fencing_token: u64,
    route_incarnation: u64,
}

macro_rules! route_accessor {
    ($name:ident, $field:ident, $type:ty) => {
        pub fn $name(&self) -> $type {
            self.$field
        }
    };
}

impl EventEnvelopeRouteV1 {
    pub fn active_ruleset_key(&self) -> &str {
        &self.active_ruleset_key
    }

    pub fn active_ruleset_content_hash(&self) -> &str {
        &self.active_ruleset_content_hash
    }

    pub fn execution_ruleset_content_hash(&self) -> &str {
        &self.execution_ruleset_content_hash
    }

    pub fn binding_fingerprint(&self) -> &str {
        &self.binding_fingerprint
    }

    pub fn process_instance_id(&self) -> &str {
        &self.process_instance_id
    }

    pub fn route_attestation_digest(&self) -> &str {
        &self.route_attestation_digest
    }

    pub fn gateway_shard_identity(&self) -> &str {
        &self.gateway_shard_identity
    }

    pub fn runtime_build_revision(&self) -> &str {
        &self.runtime_build_revision
    }

    route_accessor!(active_ruleset_version, active_ruleset_version, u32);
    route_accessor!(execution_ruleset_version, execution_ruleset_version, u32);
    route_accessor!(binding_revision, binding_revision, u64);
    route_accessor!(runtime_generation, runtime_generation, u64);
    route_accessor!(serving_lease_epoch, serving_lease_epoch, u64);
    route_accessor!(serving_lease_revision, serving_lease_revision, u64);
    route_accessor!(gateway_owner_lease_epoch, gateway_owner_lease_epoch, u64);
    route_accessor!(gateway_owner_revision, gateway_owner_revision, u64);
    route_accessor!(route_fencing_token, route_fencing_token, u64);
    route_accessor!(route_incarnation, route_incarnation, u64);
}

/// Exact serving authority captured when the durable receipt was admitted. The R0 reference
/// outbox only permits claims carrying this same fence snapshot; successor authority validation
/// belongs to the future integrated serving adapter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutboxDispatchAuthorityV1 {
    process_instance_id: String,
    gateway_shard_identity: String,
    gateway_owner_lease_epoch: u64,
    gateway_owner_revision: u64,
    runtime_build_revision: String,
    serving_lease_epoch: u64,
    serving_lease_revision: u64,
    route_fencing_token: u64,
    route_incarnation: u64,
}

impl OutboxDispatchAuthorityV1 {
    pub(crate) fn from_envelope(envelope: &EventEnvelopeV1) -> Self {
        let route = envelope.route();
        Self {
            process_instance_id: route.process_instance_id.clone(),
            gateway_shard_identity: route.gateway_shard_identity.clone(),
            gateway_owner_lease_epoch: route.gateway_owner_lease_epoch,
            gateway_owner_revision: route.gateway_owner_revision,
            runtime_build_revision: route.runtime_build_revision.clone(),
            serving_lease_epoch: route.serving_lease_epoch,
            serving_lease_revision: route.serving_lease_revision,
            route_fencing_token: route.route_fencing_token,
            route_incarnation: route.route_incarnation,
        }
    }
}

/// A stateful event envelope can only be created from an exact verified interaction request.
/// It intentionally has no `Debug` implementation because normalized modal values may be private.
#[derive(Clone, PartialEq, Eq)]
pub struct EventEnvelopeV1 {
    schema_version: u16,
    kind: String,
    receipt_identity: InteractionReceiptIdentityV1,
    request_digest: InteractionRequestDigestV1,
    claim_root_digest: InteractionReceiptClaimRootDigestV1,
    program: StatefulProgramIdentityV1,
    route: EventEnvelopeRouteV1,
    scope: EventEnvelopeScopeV1,
    trigger: TriggerV1,
    normalized_inputs: BTreeMap<String, String>,
}

impl EventEnvelopeV1 {
    pub(crate) fn from_compiled_bundle(
        bundle: &CompiledStatefulBundleV1,
        publication: &StatefulBundlePublicationBindingV1,
        verified: VerifiedInteractionRequestV1,
    ) -> Result<Self, EventEnvelopeErrorV1> {
        let route_projection = verified_legacy_route_projection(bundle, &verified)?;
        if publication.bundle_digest != bundle.bundle_digest().to_hex()
            || publication.compilation_binding_digest != bundle.binding_digest().to_hex()
            || publication.union_source_map_digest != bundle.union_source_map_digest().to_hex()
            || publication.source_spec_digest
                != bundle.stateful_artifact().source().digest().to_hex()
            || publication.stateful_artifact_digest != bundle.stateful_artifact_digest().to_hex()
            || publication.state_schema_digest != bundle.state_schema_digest().to_hex()
            || publication.program_key != route_projection.program_key
            || publication.execution_ruleset_version != route_projection.execution_ruleset_version
            || publication.filtered_legacy_content_hash
                != route_projection.filtered_legacy_content_hash
            || publication.claim_root_digest != route_projection.claim_root_digest
        {
            return Err(EventEnvelopeErrorV1::PublicationAuthorityMismatch);
        }
        let program = StatefulProgramIdentityV1::from_compiled_bundle(bundle, publication)
            .map_err(|_| EventEnvelopeErrorV1::ProgramIdentity)?;
        let envelope =
            Self::from_program_and_verified_request(bundle.source_spec(), program, verified)?;
        if envelope.claim_root_digest.as_str() != publication.claim_root_digest {
            return Err(EventEnvelopeErrorV1::ProgramIdentity);
        }
        Ok(envelope)
    }

    #[cfg(test)]
    pub(crate) fn from_verified_request(
        spec: &StatefulSpecV1,
        program: StatefulProgramIdentityV1,
        verified: VerifiedInteractionRequestV1,
    ) -> Result<Self, EventEnvelopeErrorV1> {
        Self::from_program_and_verified_request(spec, program, verified)
    }

    fn from_program_and_verified_request(
        spec: &StatefulSpecV1,
        program: StatefulProgramIdentityV1,
        verified: VerifiedInteractionRequestV1,
    ) -> Result<Self, EventEnvelopeErrorV1> {
        let expected_spec_digest =
            stateful_spec_digest_v1(spec).map_err(|_| EventEnvelopeErrorV1::ProgramIdentity)?;
        if program.program_key != spec.key || program.source_spec_digest != expected_spec_digest {
            return Err(EventEnvelopeErrorV1::ProgramIdentity);
        }

        let claim_root = verified.claim_root();
        let authoritative = claim_root.route();
        let product_scope = authoritative.scope();
        let process = authoritative.process_identity();
        let target = &process.target;
        if target.ruleset_key.as_str() != program.program_key {
            return Err(EventEnvelopeErrorV1::RouteMismatch);
        }

        let (execution_version, execution_hash, instance_id, instance_manifest_digest) =
            match authoritative.execution_route() {
                InteractionExecutionRouteV1::Static {
                    ruleset_version,
                    ruleset_content_hash,
                } => (
                    ruleset_version.get(),
                    ruleset_content_hash.to_hex(),
                    None,
                    None,
                ),
                InteractionExecutionRouteV1::Instance {
                    instance_id,
                    pinned_ruleset_version,
                    pinned_ruleset_content_hash,
                    resource_manifest_digest,
                } => (
                    pinned_ruleset_version.get(),
                    pinned_ruleset_content_hash.to_hex(),
                    Some(instance_id.as_str().to_string()),
                    Some(resource_manifest_digest.as_str().to_string()),
                ),
            };
        let legacy = &program.filtered_legacy_ruleset;
        if legacy.key != target.ruleset_key.as_str()
            || legacy.version != execution_version
            || legacy.content_hash != execution_hash
        {
            return Err(EventEnvelopeErrorV1::RouteMismatch);
        }

        let trigger = derive_trigger(
            &verified,
            target.guild_id.0,
            target.ruleset_key.as_str(),
            instance_id.as_deref(),
        )?;
        let normalized_inputs =
            normalize_stateful_event_inputs_v1(spec, &trigger, verified.inputs())
                .map_err(EventEnvelopeErrorV1::InvalidEvent)?;
        let serving = authoritative.serving_identity();

        Ok(Self {
            schema_version: EVENT_ENVELOPE_SCHEMA_VERSION_V1,
            kind: EVENT_ENVELOPE_KIND_V1.to_string(),
            receipt_identity: claim_root.identity(),
            request_digest: claim_root.request_digest().clone(),
            claim_root_digest: build_interaction_receipt_claim_root_digest_v1(claim_root),
            program,
            route: EventEnvelopeRouteV1 {
                active_ruleset_key: target.ruleset_key.as_str().to_string(),
                active_ruleset_version: target.version.get(),
                active_ruleset_content_hash: target.content_hash.to_hex(),
                execution_ruleset_version: execution_version,
                execution_ruleset_content_hash: execution_hash,
                binding_revision: target.binding_revision.get(),
                binding_fingerprint: target.binding_fingerprint.as_str().to_string(),
                runtime_generation: process.runtime_generation.get(),
                process_instance_id: process.process_instance_id.as_str().to_string(),
                route_attestation_digest: serving.attestation_digest().as_str().to_string(),
                serving_lease_epoch: serving.lease_epoch().get(),
                serving_lease_revision: serving.lease_revision().get(),
                gateway_shard_identity: serving.gateway_shard_identity().as_str().to_string(),
                gateway_owner_lease_epoch: serving.gateway_owner_lease_epoch().get(),
                gateway_owner_revision: serving.gateway_owner_revision().get(),
                runtime_build_revision: serving.runtime_build_revision().as_str().to_string(),
                route_fencing_token: serving.route_fencing_token().get(),
                route_incarnation: serving.route_incarnation().get(),
            },
            scope: EventEnvelopeScopeV1 {
                tenant_id: product_scope.tenant_id().as_str().to_string(),
                installation_id: product_scope.installation_id().as_str().to_string(),
                deployment_id: product_scope.deployment_id().as_str().to_string(),
                guild_id: verified.guild_id().0,
                channel_id: verified.channel_id().0,
                actor_user_id: verified.actor_id().0,
                instance_id,
                instance_manifest_digest,
            },
            trigger,
            normalized_inputs,
        })
    }

    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.receipt_identity
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        &self.request_digest
    }

    pub fn claim_root_digest(&self) -> &InteractionReceiptClaimRootDigestV1 {
        &self.claim_root_digest
    }

    pub fn program(&self) -> &StatefulProgramIdentityV1 {
        &self.program
    }

    pub fn route(&self) -> &EventEnvelopeRouteV1 {
        &self.route
    }

    pub fn scope(&self) -> &EventEnvelopeScopeV1 {
        &self.scope
    }

    pub fn trigger(&self) -> &TriggerV1 {
        &self.trigger
    }

    pub fn normalized_inputs(&self) -> &BTreeMap<String, String> {
        &self.normalized_inputs
    }
}

impl Drop for EventEnvelopeV1 {
    fn drop(&mut self) {
        for value in self.normalized_inputs.values_mut() {
            value.zeroize();
        }
    }
}

fn derive_trigger(
    verified: &VerifiedInteractionRequestV1,
    route_guild_id: u64,
    route_ruleset_key: &str,
    route_instance_id: Option<&str>,
) -> Result<TriggerV1, EventEnvelopeErrorV1> {
    let parsed = decode_interaction_custom_id_v1(verified.custom_id())
        .map_err(|_| EventEnvelopeErrorV1::RouteMismatch)?;
    match (route_instance_id, parsed) {
        (
            None,
            ParsedInteractionCustomIdV1::Component {
                guild_id,
                ruleset_key,
                kind,
                key,
            },
        ) => {
            if guild_id.0 != route_guild_id
                || ruleset_key != route_ruleset_key
                || !valid_identifier(&key)
            {
                return Err(EventEnvelopeErrorV1::RouteMismatch);
            }
            match (kind, verified.kind()) {
                (InteractionComponentKindV1::Button, VerifiedInteractionRequestKindV1::Button) => {
                    Ok(TriggerV1::ButtonClick { trigger_id: key })
                }
                (
                    InteractionComponentKindV1::Modal,
                    VerifiedInteractionRequestKindV1::ModalSubmit,
                ) => Ok(TriggerV1::ModalSubmit { modal_id: key }),
                _ => Err(EventEnvelopeErrorV1::RouteMismatch),
            }
        }
        (
            Some(expected_instance),
            ParsedInteractionCustomIdV1::InstanceAction {
                instance_id,
                action: action_id,
            },
        ) => {
            if verified.kind() != VerifiedInteractionRequestKindV1::Button
                || instance_id != expected_instance
                || !valid_identifier(&action_id)
            {
                return Err(EventEnvelopeErrorV1::RouteMismatch);
            }
            Ok(TriggerV1::InstanceAction { action_id })
        }
        _ => Err(EventEnvelopeErrorV1::RouteMismatch),
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EventEnvelopeDigestV1(String);

impl EventEnvelopeDigestV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, EventEnvelopeErrorV1> {
        let value = value.into();
        if valid_digest(&value) {
            Ok(Self(value))
        } else {
            Err(EventEnvelopeErrorV1::InvalidDigest)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn event_envelope_digest_v1(envelope: &EventEnvelopeV1) -> EventEnvelopeDigestV1 {
    let mut hasher = Sha256::new();
    hasher.update(EVENT_ENVELOPE_DOMAIN_V1);
    hasher.update(envelope.schema_version.to_be_bytes());
    append_string(&mut hasher, &envelope.kind);
    hasher.update(
        envelope
            .receipt_identity
            .application_id()
            .get()
            .to_be_bytes(),
    );
    hasher.update(
        envelope
            .receipt_identity
            .interaction_id()
            .get()
            .to_be_bytes(),
    );
    append_string(&mut hasher, envelope.request_digest.as_str());
    append_string(&mut hasher, envelope.claim_root_digest.as_str());
    let program = &envelope.program;
    append_string(&mut hasher, &program.program_key);
    append_string(&mut hasher, &program.source_spec_digest.to_hex());
    append_string(&mut hasher, &program.bundle_digest);
    append_string(&mut hasher, &program.compilation_binding_digest);
    append_string(&mut hasher, &program.union_source_map_digest);
    append_string(&mut hasher, program.stateful_artifact_digest.as_str());
    append_string(&mut hasher, program.state_schema_digest.as_str());
    hasher.update(program.compiler_revision.to_be_bytes());
    hasher.update(program.evaluator_revision.to_be_bytes());
    append_string(&mut hasher, &program.filtered_legacy_ruleset.key);
    hasher.update(program.filtered_legacy_ruleset.version.to_be_bytes());
    append_string(&mut hasher, &program.filtered_legacy_ruleset.content_hash);
    let route = &envelope.route;
    append_string(&mut hasher, &route.active_ruleset_key);
    hasher.update(route.active_ruleset_version.to_be_bytes());
    append_string(&mut hasher, &route.active_ruleset_content_hash);
    hasher.update(route.execution_ruleset_version.to_be_bytes());
    append_string(&mut hasher, &route.execution_ruleset_content_hash);
    hasher.update(route.binding_revision.to_be_bytes());
    append_string(&mut hasher, &route.binding_fingerprint);
    hasher.update(route.runtime_generation.to_be_bytes());
    append_string(&mut hasher, &route.process_instance_id);
    append_string(&mut hasher, &route.route_attestation_digest);
    hasher.update(route.serving_lease_epoch.to_be_bytes());
    hasher.update(route.serving_lease_revision.to_be_bytes());
    append_string(&mut hasher, &route.gateway_shard_identity);
    hasher.update(route.gateway_owner_lease_epoch.to_be_bytes());
    hasher.update(route.gateway_owner_revision.to_be_bytes());
    append_string(&mut hasher, &route.runtime_build_revision);
    hasher.update(route.route_fencing_token.to_be_bytes());
    hasher.update(route.route_incarnation.to_be_bytes());
    let scope = &envelope.scope;
    append_string(&mut hasher, &scope.tenant_id);
    append_string(&mut hasher, &scope.installation_id);
    append_string(&mut hasher, &scope.deployment_id);
    hasher.update(scope.guild_id.to_be_bytes());
    hasher.update(scope.channel_id.to_be_bytes());
    hasher.update(scope.actor_user_id.to_be_bytes());
    append_optional_string(&mut hasher, scope.instance_id.as_deref());
    append_optional_string(&mut hasher, scope.instance_manifest_digest.as_deref());
    match &envelope.trigger {
        TriggerV1::ButtonClick { trigger_id } => {
            hasher.update([0]);
            append_string(&mut hasher, trigger_id);
        }
        TriggerV1::ModalSubmit { modal_id } => {
            hasher.update([1]);
            append_string(&mut hasher, modal_id);
        }
        TriggerV1::InstanceAction { action_id } => {
            hasher.update([2]);
            append_string(&mut hasher, action_id);
        }
    }
    hasher.update((envelope.normalized_inputs.len() as u64).to_be_bytes());
    for (key, value) in &envelope.normalized_inputs {
        append_string(&mut hasher, key);
        append_string(&mut hasher, value);
    }
    EventEnvelopeDigestV1(lower_hex(hasher.finalize().as_slice()))
}

#[derive(Debug, thiserror::Error)]
pub enum EventEnvelopeErrorV1 {
    #[error("stateful program identity is invalid for this spec")]
    ProgramIdentity,
    #[error("verified interaction custom ID does not match its exact authoritative route")]
    RouteMismatch,
    #[error("verified interaction is not a valid event for the stateful program")]
    InvalidEvent(#[source] StatefulEventNormalizationErrorV1),
    #[error("verified route lacks an exact authoritative publication binding for this bundle")]
    PublicationAuthorityMismatch,
    #[error("stateful event envelope digest is invalid")]
    InvalidDigest,
}

struct VerifiedLegacyRouteProjectionV1 {
    program_key: String,
    execution_ruleset_version: u32,
    filtered_legacy_content_hash: String,
    claim_root_digest: String,
}

fn verified_legacy_route_projection(
    bundle: &CompiledStatefulBundleV1,
    verified: &VerifiedInteractionRequestV1,
) -> Result<VerifiedLegacyRouteProjectionV1, EventEnvelopeErrorV1> {
    let authoritative = verified.claim_root().route();
    let target = &authoritative.process_identity().target;
    let bundle_target = bundle.filtered_legacy_target();
    if target.ruleset_key.as_str() != bundle_target.ruleset_key
        || bundle.stateful_artifact().program_key() != bundle_target.ruleset_key
    {
        return Err(EventEnvelopeErrorV1::RouteMismatch);
    }
    let (version, content_hash) = match authoritative.execution_route() {
        InteractionExecutionRouteV1::Static {
            ruleset_version,
            ruleset_content_hash,
        } => (ruleset_version.get(), ruleset_content_hash.to_hex()),
        InteractionExecutionRouteV1::Instance {
            pinned_ruleset_version,
            pinned_ruleset_content_hash,
            ..
        } => (
            pinned_ruleset_version.get(),
            pinned_ruleset_content_hash.to_hex(),
        ),
    };
    if content_hash != bundle_target.content_hash.to_hex() {
        return Err(EventEnvelopeErrorV1::RouteMismatch);
    }
    Ok(VerifiedLegacyRouteProjectionV1 {
        program_key: bundle_target.ruleset_key.clone(),
        execution_ruleset_version: version,
        filtered_legacy_content_hash: content_hash,
        claim_root_digest: build_interaction_receipt_claim_root_digest_v1(verified.claim_root())
            .as_str()
            .to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulProgramIdentityErrorV1 {
    #[error("stateful program digest is invalid")]
    InvalidDigest,
    #[error("filtered legacy RuleSet identity is invalid")]
    InvalidLegacyRuleSet,
    #[error("stateful program key does not match the filtered legacy RuleSet")]
    ProgramKeyMismatch,
    #[error("stateful program source spec is invalid")]
    InvalidSpec,
    #[error("stateful program does not match its verified bundle publication binding")]
    PublicationMismatch,
}

fn append_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn append_optional_string(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            append_string(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
