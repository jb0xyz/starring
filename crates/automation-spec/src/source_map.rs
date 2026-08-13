use std::fmt::{Display, Formatter};

use automation_ruleset::{
    content_hash, RuleSetContentHash, RuleSetKey, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::InteractionRuleSet;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::canonical::{
    automation_spec_digest_v1, canonical_json_bytes, framed_sha256, AutomationSpecDigestErrorV1,
    AutomationSpecDigestV1,
};
use crate::model::AutomationSpecV1;
use crate::validate::{lower_shape, validate_automation_spec_v1, AutomationSpecValidationErrorV1};

pub const AUTOMATION_COMPILER_REVISION_V1: u32 = 1;
pub const AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1: u16 = 1;
pub const AUTOMATION_SOURCE_MAP_KIND_V1: &str = "starring.automation-source-map.v1";
pub const AUTOMATION_COMPILATION_BINDING_FORMAT_VERSION_V1: u16 = 1;
pub const AUTOMATION_COMPILATION_BINDING_KIND_V1: &str =
    "starring.automation-compilation-binding.v1";

const SOURCE_MAP_DOMAIN_V1: &[u8] = b"starring.automation_source_map.v1\0";
const COMPILATION_BINDING_DOMAIN_V1: &[u8] = b"starring.automation_compilation_binding.v1\0";

macro_rules! digest_type {
    ($name:ident, $message:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            pub fn to_hex(self) -> String {
                let mut output = String::with_capacity(64);
                for byte in self.0 {
                    use std::fmt::Write;
                    write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
                }
                output
            }

            pub fn parse(value: &str) -> Option<Self> {
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return None;
                }
                let mut bytes = [0u8; 32];
                for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
                    let high = (pair[0] as char).to_digit(16)? as u8;
                    let low = (pair[1] as char).to_digit(16)? as u8;
                    bytes[index] = (high << 4) | low;
                }
                Some(Self(bytes))
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.to_hex())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).ok_or_else(|| serde::de::Error::custom($message))
            }
        }
    };
}

digest_type!(
    AutomationSourceMapDigestV1,
    "expected a 64-character lowercase source-map SHA-256 digest"
);
digest_type!(
    AutomationCompilationBindingDigestV1,
    "expected a 64-character lowercase compilation-binding SHA-256 digest"
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecIdentityV1 {
    pub schema_version: u16,
    pub digest: AutomationSpecDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRuleSetIdentityV1 {
    pub ruleset_key: String,
    pub schema_version: u32,
    pub content_hash: RuleSetContentHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSourceMapV1 {
    pub schema_version: u16,
    pub kind: String,
    pub compiler_revision: u32,
    pub source: AutomationSpecIdentityV1,
    pub target: AutomationRuleSetIdentityV1,
    pub workflows: Vec<WorkflowSourceMapV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSourceMapV1 {
    pub workflow_id: String,
    pub source_workflow_index: u32,
    pub target_rule_index: u32,
    pub target_rule_key: String,
    pub actions: Vec<ActionSourceMapV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSourceMapV1 {
    pub action_node_id: String,
    pub source_action_index: u32,
    pub target_action_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompilationBindingV1 {
    pub format_version: u16,
    pub kind: String,
    pub compiler_revision: u32,
    pub source: AutomationSpecIdentityV1,
    pub target: AutomationRuleSetIdentityV1,
    pub source_map_schema_version: u16,
    pub source_map_digest: AutomationSourceMapDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompiledTargetArtifactsV1 {
    pub ruleset: InteractionRuleSet,
    pub target: AutomationRuleSetIdentityV1,
    pub source_map: AutomationSourceMapV1,
    pub source_map_digest: AutomationSourceMapDigestV1,
    pub binding: AutomationCompilationBindingV1,
    pub binding_digest: AutomationCompilationBindingDigestV1,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationCompilationIdentityErrorV1 {
    #[error("automation spec is invalid")]
    InvalidSpec(#[from] AutomationSpecValidationErrorV1),
    #[error("automation spec identity could not be computed")]
    Spec(#[from] AutomationSpecDigestErrorV1),
    #[error("ruleset key is invalid")]
    RuleSetKey,
    #[error("compiled ruleset identity could not be computed")]
    RuleSetIdentity,
    #[error("source map or compilation binding could not be encoded")]
    Encoding,
    #[error("source map or compilation binding is not canonical")]
    NonCanonical,
    #[error("source map or compilation binding does not match the compiled automation")]
    Mismatch,
    #[error("conditional automation cannot be bound to interaction runtime V1")]
    ConditionalRuntimeUnavailable,
}

pub(crate) fn build_compiled_target_artifacts_v1(
    spec: &AutomationSpecV1,
) -> Result<CompiledTargetArtifactsV1, AutomationCompilationIdentityErrorV1> {
    validate_automation_spec_v1(spec)?;
    if spec
        .workflows
        .iter()
        .any(|workflow| !workflow.condition.is_unconditional())
    {
        return Err(AutomationCompilationIdentityErrorV1::ConditionalRuntimeUnavailable);
    }
    RuleSetKey::parse(&spec.key).map_err(|_| AutomationCompilationIdentityErrorV1::RuleSetKey)?;
    let ruleset = lower_shape(spec);
    let content_hash = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset)
        .map_err(|_| AutomationCompilationIdentityErrorV1::RuleSetIdentity)?;
    let source = AutomationSpecIdentityV1 {
        schema_version: spec.schema_version,
        digest: automation_spec_digest_v1(spec)?,
    };
    let target = AutomationRuleSetIdentityV1 {
        ruleset_key: spec.key.clone(),
        schema_version: CURRENT_RULESET_SCHEMA_VERSION.get(),
        content_hash,
    };
    let source_map = AutomationSourceMapV1 {
        schema_version: AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1,
        kind: AUTOMATION_SOURCE_MAP_KIND_V1.to_string(),
        compiler_revision: AUTOMATION_COMPILER_REVISION_V1,
        source: source.clone(),
        target: target.clone(),
        workflows: spec
            .workflows
            .iter()
            .enumerate()
            .map(|(workflow_index, workflow)| WorkflowSourceMapV1 {
                workflow_id: workflow.id.clone(),
                source_workflow_index: workflow_index as u32,
                target_rule_index: workflow_index as u32,
                target_rule_key: workflow.id.clone(),
                actions: workflow
                    .actions
                    .iter()
                    .enumerate()
                    .map(|(action_index, action)| ActionSourceMapV1 {
                        action_node_id: action.id.clone(),
                        source_action_index: action_index as u32,
                        target_action_index: action_index as u32,
                    })
                    .collect(),
            })
            .collect(),
    };
    let source_map_digest = automation_source_map_digest_v1(&source_map)?;
    let binding = AutomationCompilationBindingV1 {
        format_version: AUTOMATION_COMPILATION_BINDING_FORMAT_VERSION_V1,
        kind: AUTOMATION_COMPILATION_BINDING_KIND_V1.to_string(),
        compiler_revision: AUTOMATION_COMPILER_REVISION_V1,
        source,
        target: target.clone(),
        source_map_schema_version: AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1,
        source_map_digest,
    };
    let binding_digest = automation_compilation_binding_digest_v1(&binding)?;
    Ok(CompiledTargetArtifactsV1 {
        ruleset,
        target,
        source_map,
        source_map_digest,
        binding,
        binding_digest,
    })
}

pub fn canonical_automation_source_map_bytes_v1(
    source_map: &AutomationSourceMapV1,
) -> Result<Vec<u8>, AutomationCompilationIdentityErrorV1> {
    canonical_json_bytes(source_map).map_err(|_| AutomationCompilationIdentityErrorV1::Encoding)
}

pub fn canonical_automation_compilation_binding_bytes_v1(
    binding: &AutomationCompilationBindingV1,
) -> Result<Vec<u8>, AutomationCompilationIdentityErrorV1> {
    canonical_json_bytes(binding).map_err(|_| AutomationCompilationIdentityErrorV1::Encoding)
}

pub fn decode_canonical_automation_source_map_v1(
    bytes: &[u8],
) -> Result<AutomationSourceMapV1, AutomationCompilationIdentityErrorV1> {
    let value = serde_json::from_slice::<AutomationSourceMapV1>(bytes)
        .map_err(|_| AutomationCompilationIdentityErrorV1::NonCanonical)?;
    if canonical_automation_source_map_bytes_v1(&value)? != bytes {
        return Err(AutomationCompilationIdentityErrorV1::NonCanonical);
    }
    Ok(value)
}

pub fn decode_canonical_automation_compilation_binding_v1(
    bytes: &[u8],
) -> Result<AutomationCompilationBindingV1, AutomationCompilationIdentityErrorV1> {
    let value = serde_json::from_slice::<AutomationCompilationBindingV1>(bytes)
        .map_err(|_| AutomationCompilationIdentityErrorV1::NonCanonical)?;
    if canonical_automation_compilation_binding_bytes_v1(&value)? != bytes {
        return Err(AutomationCompilationIdentityErrorV1::NonCanonical);
    }
    Ok(value)
}

pub fn automation_source_map_digest_v1(
    source_map: &AutomationSourceMapV1,
) -> Result<AutomationSourceMapDigestV1, AutomationCompilationIdentityErrorV1> {
    let bytes = canonical_automation_source_map_bytes_v1(source_map)?;
    Ok(AutomationSourceMapDigestV1(framed_sha256(
        SOURCE_MAP_DOMAIN_V1,
        &bytes,
    )))
}

pub fn automation_compilation_binding_digest_v1(
    binding: &AutomationCompilationBindingV1,
) -> Result<AutomationCompilationBindingDigestV1, AutomationCompilationIdentityErrorV1> {
    let bytes = canonical_automation_compilation_binding_bytes_v1(binding)?;
    Ok(AutomationCompilationBindingDigestV1(framed_sha256(
        COMPILATION_BINDING_DOMAIN_V1,
        &bytes,
    )))
}

pub fn validate_automation_compilation_v1(
    spec: &AutomationSpecV1,
    ruleset: &InteractionRuleSet,
    source_map: &AutomationSourceMapV1,
    binding: &AutomationCompilationBindingV1,
) -> Result<(), AutomationCompilationIdentityErrorV1> {
    let expected = build_compiled_target_artifacts_v1(spec)?;
    if &expected.ruleset != ruleset
        || &expected.source_map != source_map
        || &expected.binding != binding
        || source_map.schema_version != AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1
        || source_map.kind != AUTOMATION_SOURCE_MAP_KIND_V1
        || source_map.compiler_revision != AUTOMATION_COMPILER_REVISION_V1
        || binding.format_version != AUTOMATION_COMPILATION_BINDING_FORMAT_VERSION_V1
        || binding.kind != AUTOMATION_COMPILATION_BINDING_KIND_V1
        || binding.compiler_revision != AUTOMATION_COMPILER_REVISION_V1
        || binding.source_map_schema_version != AUTOMATION_SOURCE_MAP_SCHEMA_VERSION_V1
        || automation_source_map_digest_v1(source_map)? != binding.source_map_digest
    {
        return Err(AutomationCompilationIdentityErrorV1::Mismatch);
    }
    Ok(())
}
