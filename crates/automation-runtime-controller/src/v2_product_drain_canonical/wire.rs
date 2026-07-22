use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_runtime_convergence::{
    BindingRevision, DeploymentId, DeploymentRevision, InstallationId, RuntimeDeploymentTargetV1,
    TenantId,
};
use discord_model::GuildId;
use resource_resolution::ResourceBindingFingerprint;
use serde::{Deserialize, Serialize};

use super::{
    validate_drain, validate_product, RuntimeProductDrainCanonicalErrorV2,
    RuntimeProductDrainCanonicalFieldV2, RuntimeProductDrainCanonicalRootV2,
    DRAIN_INTENT_MAX_OCTETS, PRODUCT_MUTATION_MAX_OCTETS,
};
use crate::v2_canonical_value::{RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2};
use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentIdV2,
    RuntimeDrainIntentKeyV2, RuntimeDrainIntentPreimageV2, RuntimeProductMutationDigestV2,
    RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2, RuntimeProductOperationIdV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeServingSlotV2,
};

const FORMAT_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductMutationWireV2 {
    format_version: u8,
    operation_id: String,
    scope: DeploymentScopeWireV2,
    expected_revision: u64,
    slot: ServingSlotWireV2,
    expected_target: DeploymentTargetWireV2,
    mutation_kind: String,
    product_semantic_request_digest: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainIntentWireV2 {
    format_version: u8,
    key: DrainIntentKeyWireV2,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DrainIntentKeyWireV2 {
    intent_id: String,
    product_operation_id: String,
    product_mutation_digest: String,
    scope: DeploymentScopeWireV2,
    expected_revision: u64,
    slot: ServingSlotWireV2,
    expected_target: DeploymentTargetWireV2,
    mutation_kind: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentScopeWireV2 {
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServingSlotWireV2 {
    guild_id: String,
    ruleset_key: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeploymentTargetWireV2 {
    guild_id: String,
    ruleset_key: String,
    version: u32,
    content_hash: String,
    binding_revision: u64,
    binding_fingerprint: String,
}

pub(super) fn encode_product_mutation(
    preimage: &RuntimeProductMutationPreimageV2,
) -> Result<Vec<u8>, RuntimeProductDrainCanonicalErrorV2> {
    validate_product(preimage)?;
    let root = RuntimeProductDrainCanonicalRootV2::ProductMutation;
    let wire = ProductMutationWireV2 {
        format_version: FORMAT_VERSION,
        operation_id: preimage.operation_id.as_str().to_owned(),
        scope: encode_scope(&preimage.scope),
        expected_revision: persistence_u64(
            preimage.expected_revision.get(),
            root,
            RuntimeProductDrainCanonicalFieldV2::ExpectedRevision,
        )?,
        slot: encode_slot(&preimage.slot, root)?,
        expected_target: encode_target(&preimage.expected_target, root)?,
        mutation_kind: mutation_kind_tag(preimage.mutation_kind).to_owned(),
        product_semantic_request_digest: preimage
            .product_semantic_request_digest
            .as_str()
            .to_owned(),
    };
    encode_root(&wire, root, PRODUCT_MUTATION_MAX_OCTETS)
}

pub(super) fn decode_product_mutation(
    encoded: &[u8],
) -> Result<RuntimeProductMutationPreimageV2, RuntimeProductDrainCanonicalErrorV2> {
    let root = RuntimeProductDrainCanonicalRootV2::ProductMutation;
    ensure_size(encoded, root, PRODUCT_MUTATION_MAX_OCTETS)?;
    let wire = serde_json::from_slice::<ProductMutationWireV2>(encoded)
        .map_err(|_| RuntimeProductDrainCanonicalErrorV2::Decoding { root })?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeProductDrainCanonicalErrorV2::UnsupportedFormatVersion { root });
    }
    let preimage = RuntimeProductMutationPreimageV2 {
        operation_id: RuntimeProductOperationIdV2::parse(wire.operation_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::OperationId))?,
        scope: decode_scope(wire.scope, root)?,
        expected_revision: decode_revision(
            wire.expected_revision,
            root,
            RuntimeProductDrainCanonicalFieldV2::ExpectedRevision,
        )?,
        slot: decode_slot(wire.slot, root)?,
        expected_target: decode_target(wire.expected_target, root)?,
        mutation_kind: decode_mutation_kind(&wire.mutation_kind, root)?,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2::parse(
            wire.product_semantic_request_digest,
        )
        .map_err(|_| {
            invalid(
                root,
                RuntimeProductDrainCanonicalFieldV2::ProductSemanticRequestDigest,
            )
        })?,
    };
    validate_product(&preimage)?;
    let canonical = encode_product_mutation(&preimage)?;
    if canonical != encoded {
        return Err(RuntimeProductDrainCanonicalErrorV2::NonCanonicalEncoding { root });
    }
    Ok(preimage)
}

pub(super) fn encode_drain_intent(
    preimage: &RuntimeDrainIntentPreimageV2,
) -> Result<Vec<u8>, RuntimeProductDrainCanonicalErrorV2> {
    validate_drain(preimage)?;
    let root = RuntimeProductDrainCanonicalRootV2::DrainIntent;
    let key = &preimage.key;
    let wire = DrainIntentWireV2 {
        format_version: FORMAT_VERSION,
        key: DrainIntentKeyWireV2 {
            intent_id: key.intent_id.as_str().to_owned(),
            product_operation_id: key.product_operation_id.as_str().to_owned(),
            product_mutation_digest: key.product_mutation_digest.as_str().to_owned(),
            scope: encode_scope(&key.scope),
            expected_revision: persistence_u64(
                key.expected_revision.get(),
                root,
                RuntimeProductDrainCanonicalFieldV2::ExpectedRevision,
            )?,
            slot: encode_slot(&key.slot, root)?,
            expected_target: encode_target(&key.expected_target, root)?,
            mutation_kind: mutation_kind_tag(key.mutation_kind).to_owned(),
        },
    };
    encode_root(&wire, root, DRAIN_INTENT_MAX_OCTETS)
}

pub(super) fn decode_drain_intent(
    encoded: &[u8],
) -> Result<RuntimeDrainIntentPreimageV2, RuntimeProductDrainCanonicalErrorV2> {
    let root = RuntimeProductDrainCanonicalRootV2::DrainIntent;
    ensure_size(encoded, root, DRAIN_INTENT_MAX_OCTETS)?;
    let wire = serde_json::from_slice::<DrainIntentWireV2>(encoded)
        .map_err(|_| RuntimeProductDrainCanonicalErrorV2::Decoding { root })?;
    if wire.format_version != FORMAT_VERSION {
        return Err(RuntimeProductDrainCanonicalErrorV2::UnsupportedFormatVersion { root });
    }
    let key = RuntimeDrainIntentKeyV2 {
        intent_id: RuntimeDrainIntentIdV2::parse(wire.key.intent_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::IntentId))?,
        product_operation_id: RuntimeProductOperationIdV2::parse(wire.key.product_operation_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::OperationId))?,
        product_mutation_digest: RuntimeProductMutationDigestV2::parse(
            wire.key.product_mutation_digest,
        )
        .map_err(|_| {
            invalid(
                root,
                RuntimeProductDrainCanonicalFieldV2::ProductMutationDigest,
            )
        })?,
        scope: decode_scope(wire.key.scope, root)?,
        expected_revision: decode_revision(
            wire.key.expected_revision,
            root,
            RuntimeProductDrainCanonicalFieldV2::ExpectedRevision,
        )?,
        slot: decode_slot(wire.key.slot, root)?,
        expected_target: decode_target(wire.key.expected_target, root)?,
        mutation_kind: decode_mutation_kind(&wire.key.mutation_kind, root)?,
    };
    let preimage = RuntimeDrainIntentPreimageV2::from_key(key);
    validate_drain(&preimage)?;
    let canonical = encode_drain_intent(&preimage)?;
    if canonical != encoded {
        return Err(RuntimeProductDrainCanonicalErrorV2::NonCanonicalEncoding { root });
    }
    Ok(preimage)
}

fn encode_scope(scope: &RuntimeDeploymentScopeV1) -> DeploymentScopeWireV2 {
    DeploymentScopeWireV2 {
        tenant_id: scope.tenant_id.as_str().to_owned(),
        installation_id: scope.installation_id.as_str().to_owned(),
        deployment_id: scope.deployment_id.as_str().to_owned(),
    }
}

fn decode_scope(
    wire: DeploymentScopeWireV2,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<RuntimeDeploymentScopeV1, RuntimeProductDrainCanonicalErrorV2> {
    Ok(RuntimeDeploymentScopeV1 {
        tenant_id: TenantId::parse(wire.tenant_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::TenantId))?,
        installation_id: InstallationId::parse(wire.installation_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::InstallationId))?,
        deployment_id: DeploymentId::parse(wire.deployment_id)
            .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::DeploymentId))?,
    })
}

fn encode_slot(
    slot: &RuntimeServingSlotV2,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<ServingSlotWireV2, RuntimeProductDrainCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::from_u64(slot.guild_id.0).map_err(|reason| {
        canonical(
            root,
            RuntimeProductDrainCanonicalFieldV2::SlotGuildId,
            reason,
        )
    })?;
    Ok(ServingSlotWireV2 {
        guild_id: guild_id.canonical_text(),
        ruleset_key: slot.ruleset_key.as_str().to_owned(),
    })
}

fn decode_slot(
    wire: ServingSlotWireV2,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<RuntimeServingSlotV2, RuntimeProductDrainCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id).map_err(|reason| {
        canonical(
            root,
            RuntimeProductDrainCanonicalFieldV2::SlotGuildId,
            reason,
        )
    })?;
    let ruleset_key = RuleSetKey::parse(&wire.ruleset_key)
        .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::SlotRuleSetKey))?;
    Ok(RuntimeServingSlotV2::new(
        GuildId(guild_id.get_u64()),
        ruleset_key,
    ))
}

fn encode_target(
    target: &RuntimeDeploymentTargetV1,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<DeploymentTargetWireV2, RuntimeProductDrainCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::from_u64(target.guild_id.0).map_err(|reason| {
        canonical(
            root,
            RuntimeProductDrainCanonicalFieldV2::TargetGuildId,
            reason,
        )
    })?;
    Ok(DeploymentTargetWireV2 {
        guild_id: guild_id.canonical_text(),
        ruleset_key: target.ruleset_key.as_str().to_owned(),
        version: target.version.get(),
        content_hash: target.content_hash.to_hex(),
        binding_revision: persistence_u64(
            target.binding_revision.get(),
            root,
            RuntimeProductDrainCanonicalFieldV2::TargetBindingRevision,
        )?,
        binding_fingerprint: target.binding_fingerprint.as_str().to_owned(),
    })
}

fn decode_target(
    wire: DeploymentTargetWireV2,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<RuntimeDeploymentTargetV1, RuntimeProductDrainCanonicalErrorV2> {
    let guild_id = RuntimeDiscordSnowflakeV2::parse_text(&wire.guild_id).map_err(|reason| {
        canonical(
            root,
            RuntimeProductDrainCanonicalFieldV2::TargetGuildId,
            reason,
        )
    })?;
    let ruleset_key = RuleSetKey::parse(&wire.ruleset_key)
        .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::TargetRuleSetKey))?;
    let version = RuleSetVersionId::new(wire.version)
        .map_err(|_| invalid(root, RuntimeProductDrainCanonicalFieldV2::TargetVersion))?;
    let content_hash = RuleSetContentHash::parse_hex(&wire.content_hash)
        .ok_or_else(|| invalid(root, RuntimeProductDrainCanonicalFieldV2::TargetContentHash))?;
    let binding_revision = decode_binding_revision(
        wire.binding_revision,
        root,
        RuntimeProductDrainCanonicalFieldV2::TargetBindingRevision,
    )?;
    let binding_fingerprint = ResourceBindingFingerprint::parse(&wire.binding_fingerprint)
        .map_err(|_| {
            invalid(
                root,
                RuntimeProductDrainCanonicalFieldV2::TargetBindingFingerprint,
            )
        })?;
    Ok(RuntimeDeploymentTargetV1 {
        guild_id: GuildId(guild_id.get_u64()),
        ruleset_key,
        version,
        content_hash,
        binding_revision,
        binding_fingerprint,
    })
}

fn decode_revision(
    value: u64,
    root: RuntimeProductDrainCanonicalRootV2,
    field: RuntimeProductDrainCanonicalFieldV2,
) -> Result<DeploymentRevision, RuntimeProductDrainCanonicalErrorV2> {
    let value = persistence_u64(value, root, field)?;
    DeploymentRevision::new(value).map_err(|_| invalid(root, field))
}

fn decode_binding_revision(
    value: u64,
    root: RuntimeProductDrainCanonicalRootV2,
    field: RuntimeProductDrainCanonicalFieldV2,
) -> Result<BindingRevision, RuntimeProductDrainCanonicalErrorV2> {
    let value = persistence_u64(value, root, field)?;
    BindingRevision::new(value).map_err(|_| invalid(root, field))
}

fn persistence_u64(
    value: u64,
    root: RuntimeProductDrainCanonicalRootV2,
    field: RuntimeProductDrainCanonicalFieldV2,
) -> Result<u64, RuntimeProductDrainCanonicalErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(RuntimePersistenceU64V2::get_u64)
        .map_err(|reason| canonical(root, field, reason))
}

fn mutation_kind_tag(kind: RuntimeProductMutationKindV2) -> &'static str {
    match kind {
        RuntimeProductMutationKindV2::Apply => "apply",
        RuntimeProductMutationKindV2::Supersede => "supersede",
        RuntimeProductMutationKindV2::Cancel => "cancel",
        RuntimeProductMutationKindV2::AuthorityChange => "authority_change",
        RuntimeProductMutationKindV2::Teardown => "teardown",
    }
}

fn decode_mutation_kind(
    value: &str,
    root: RuntimeProductDrainCanonicalRootV2,
) -> Result<RuntimeProductMutationKindV2, RuntimeProductDrainCanonicalErrorV2> {
    match value {
        "apply" => Ok(RuntimeProductMutationKindV2::Apply),
        "supersede" => Ok(RuntimeProductMutationKindV2::Supersede),
        "cancel" => Ok(RuntimeProductMutationKindV2::Cancel),
        "authority_change" => Ok(RuntimeProductMutationKindV2::AuthorityChange),
        "teardown" => Ok(RuntimeProductMutationKindV2::Teardown),
        _ => Err(invalid(
            root,
            RuntimeProductDrainCanonicalFieldV2::MutationKind,
        )),
    }
}

fn encode_root<T: Serialize>(
    wire: &T,
    root: RuntimeProductDrainCanonicalRootV2,
    maximum: usize,
) -> Result<Vec<u8>, RuntimeProductDrainCanonicalErrorV2> {
    let encoded = serde_json::to_vec(wire)
        .map_err(|_| RuntimeProductDrainCanonicalErrorV2::Encoding { root })?;
    ensure_size(&encoded, root, maximum)?;
    Ok(encoded)
}

fn ensure_size(
    encoded: &[u8],
    root: RuntimeProductDrainCanonicalRootV2,
    maximum: usize,
) -> Result<(), RuntimeProductDrainCanonicalErrorV2> {
    if encoded.len() > maximum {
        return Err(RuntimeProductDrainCanonicalErrorV2::PayloadTooLarge { root });
    }
    Ok(())
}

fn invalid(
    root: RuntimeProductDrainCanonicalRootV2,
    field: RuntimeProductDrainCanonicalFieldV2,
) -> RuntimeProductDrainCanonicalErrorV2 {
    RuntimeProductDrainCanonicalErrorV2::InvalidField { root, field }
}

fn canonical(
    root: RuntimeProductDrainCanonicalRootV2,
    field: RuntimeProductDrainCanonicalFieldV2,
    reason: RuntimeCanonicalValueErrorV2,
) -> RuntimeProductDrainCanonicalErrorV2 {
    RuntimeProductDrainCanonicalErrorV2::CanonicalValue {
        root,
        field,
        reason,
    }
}
