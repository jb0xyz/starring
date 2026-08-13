use std::collections::BTreeMap;

use automation_stateful_spec::StatefulSpecV1;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::compile::compile_stateful_spec_bundle_v1;
use crate::digest::{
    StatefulArtifactDigestV1, StatefulBundleDigestV1, StatefulCompilationBindingDigestV1,
    StatefulStateSchemaDigestV1, StatefulUnionSourceMapDigestV1,
};
use crate::model::{
    CompiledStateSchemaV1, CompiledStatefulArtifactV1, CompiledStatefulBundleV1,
    StatefulCompilationBindingV1, StatefulUnionSourceMapV1, STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
    STATEFUL_BUNDLE_FORMAT_VERSION_V1, STATEFUL_BUNDLE_KIND_V1,
};

pub const MAX_COMPILED_STATE_SCHEMA_CANONICAL_BYTES_V1: usize = 128 * 1024;
pub const MAX_STATEFUL_ARTIFACT_CANONICAL_BYTES_V1: usize = 512 * 1024;
pub const MAX_STATEFUL_UNION_SOURCE_MAP_CANONICAL_BYTES_V1: usize = 512 * 1024;
pub const MAX_STATEFUL_COMPILATION_BINDING_CANONICAL_BYTES_V1: usize = 64 * 1024;
pub const MAX_STATEFUL_BUNDLE_CANONICAL_BYTES_V1: usize = 2 * 1024 * 1024;

const STATE_SCHEMA_DOMAIN_V1: &[u8] = b"starring.compiled_state_schema.v1\0";
const STATEFUL_ARTIFACT_DOMAIN_V1: &[u8] = b"starring.stateful_artifact.v1\0";
const UNION_SOURCE_MAP_DOMAIN_V1: &[u8] = b"starring.stateful_union_source_map.v1\0";
const COMPILATION_BINDING_DOMAIN_V1: &[u8] = b"starring.stateful_compilation_binding.v1\0";
const BUNDLE_DOMAIN_V1: &[u8] = b"starring.stateful_compiled_bundle.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulCompilationIdentityErrorV1 {
    #[error("compiled stateful artifact encoding failed")]
    Encoding,
    #[error("compiled stateful artifact exceeds its canonical byte bound")]
    BoundExceeded,
    #[error("compiled stateful bundle JSON is not canonical")]
    NonCanonicalBundle,
    #[error("compiled stateful bundle source is invalid")]
    InvalidSource,
    #[error("compiled stateful bundle does not match a fresh compilation of its source")]
    BundleMismatch,
}

pub fn canonical_compiled_state_schema_bytes_v1(
    schema: &CompiledStateSchemaV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    bounded_canonical_json(schema, MAX_COMPILED_STATE_SCHEMA_CANONICAL_BYTES_V1)
}

pub fn stateful_state_schema_digest_v1(
    schema: &CompiledStateSchemaV1,
) -> Result<StatefulStateSchemaDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulStateSchemaDigestV1::from_bytes(framed_sha256(
        STATE_SCHEMA_DOMAIN_V1,
        &canonical_compiled_state_schema_bytes_v1(schema)?,
    )))
}

pub fn canonical_stateful_artifact_bytes_v1(
    artifact: &CompiledStatefulArtifactV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    bounded_canonical_json(artifact, MAX_STATEFUL_ARTIFACT_CANONICAL_BYTES_V1)
}

pub fn stateful_artifact_digest_v1(
    artifact: &CompiledStatefulArtifactV1,
) -> Result<StatefulArtifactDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulArtifactDigestV1::from_bytes(framed_sha256(
        STATEFUL_ARTIFACT_DOMAIN_V1,
        &canonical_stateful_artifact_bytes_v1(artifact)?,
    )))
}

pub fn canonical_stateful_union_source_map_bytes_v1(
    source_map: &StatefulUnionSourceMapV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    bounded_canonical_json(source_map, MAX_STATEFUL_UNION_SOURCE_MAP_CANONICAL_BYTES_V1)
}

pub fn stateful_union_source_map_digest_v1(
    source_map: &StatefulUnionSourceMapV1,
) -> Result<StatefulUnionSourceMapDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulUnionSourceMapDigestV1::from_bytes(framed_sha256(
        UNION_SOURCE_MAP_DOMAIN_V1,
        &canonical_stateful_union_source_map_bytes_v1(source_map)?,
    )))
}

pub fn canonical_stateful_compilation_binding_bytes_v1(
    binding: &StatefulCompilationBindingV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    bounded_canonical_json(binding, MAX_STATEFUL_COMPILATION_BINDING_CANONICAL_BYTES_V1)
}

pub fn stateful_compilation_binding_digest_v1(
    binding: &StatefulCompilationBindingV1,
) -> Result<StatefulCompilationBindingDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulCompilationBindingDigestV1::from_bytes(
        framed_sha256(
            COMPILATION_BINDING_DOMAIN_V1,
            &canonical_stateful_compilation_binding_bytes_v1(binding)?,
        ),
    ))
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct BundleWireRefV1<'a> {
    format_version: u16,
    kind: &'static str,
    compiler_revision: u32,
    source_spec: &'a StatefulSpecV1,
    filtered_legacy_ruleset: &'a automation_state::InteractionRuleSet,
    stateful_artifact: &'a CompiledStatefulArtifactV1,
    union_source_map: &'a StatefulUnionSourceMapV1,
    binding: &'a StatefulCompilationBindingV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UntrustedBundleWireV1 {
    format_version: u16,
    kind: String,
    compiler_revision: u32,
    source_spec: StatefulSpecV1,
    filtered_legacy_ruleset: Value,
    stateful_artifact: Value,
    union_source_map: Value,
    binding: Value,
}

pub fn canonical_stateful_bundle_bytes_v1(
    bundle: &CompiledStatefulBundleV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    canonical_bundle_parts_bytes_v1(
        &bundle.source_spec,
        &bundle.filtered_legacy_ruleset,
        &bundle.stateful_artifact,
        &bundle.union_source_map,
        &bundle.binding,
    )
}

pub(crate) fn canonical_bundle_parts_bytes_v1(
    source_spec: &StatefulSpecV1,
    filtered_legacy_ruleset: &automation_state::InteractionRuleSet,
    stateful_artifact: &CompiledStatefulArtifactV1,
    union_source_map: &StatefulUnionSourceMapV1,
    binding: &StatefulCompilationBindingV1,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    bounded_canonical_json(
        &BundleWireRefV1 {
            format_version: STATEFUL_BUNDLE_FORMAT_VERSION_V1,
            kind: STATEFUL_BUNDLE_KIND_V1,
            compiler_revision: STATEFUL_ARTIFACT_COMPILER_REVISION_V1,
            source_spec,
            filtered_legacy_ruleset,
            stateful_artifact,
            union_source_map,
            binding,
        },
        MAX_STATEFUL_BUNDLE_CANONICAL_BYTES_V1,
    )
}

pub fn stateful_bundle_digest_v1(
    bundle: &CompiledStatefulBundleV1,
) -> Result<StatefulBundleDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulBundleDigestV1::from_bytes(framed_sha256(
        BUNDLE_DOMAIN_V1,
        &canonical_stateful_bundle_bytes_v1(bundle)?,
    )))
}

pub(crate) fn stateful_bundle_parts_digest_v1(
    source_spec: &StatefulSpecV1,
    filtered_legacy_ruleset: &automation_state::InteractionRuleSet,
    stateful_artifact: &CompiledStatefulArtifactV1,
    union_source_map: &StatefulUnionSourceMapV1,
    binding: &StatefulCompilationBindingV1,
) -> Result<StatefulBundleDigestV1, StatefulCompilationIdentityErrorV1> {
    Ok(StatefulBundleDigestV1::from_bytes(framed_sha256(
        BUNDLE_DOMAIN_V1,
        &canonical_bundle_parts_bytes_v1(
            source_spec,
            filtered_legacy_ruleset,
            stateful_artifact,
            union_source_map,
            binding,
        )?,
    )))
}

/// Decodes only by validating the source, recompiling it, and comparing the exact canonical
/// bundle bytes. Generated JSON is never trusted as construction authority.
pub fn decode_canonical_stateful_bundle_v1(
    bytes: &[u8],
) -> Result<CompiledStatefulBundleV1, StatefulCompilationIdentityErrorV1> {
    if bytes.len() > MAX_STATEFUL_BUNDLE_CANONICAL_BYTES_V1 {
        return Err(StatefulCompilationIdentityErrorV1::BoundExceeded);
    }
    let untrusted = serde_json::from_slice::<UntrustedBundleWireV1>(bytes)
        .map_err(|_| StatefulCompilationIdentityErrorV1::NonCanonicalBundle)?;
    if untrusted.format_version != STATEFUL_BUNDLE_FORMAT_VERSION_V1
        || untrusted.kind != STATEFUL_BUNDLE_KIND_V1
        || untrusted.compiler_revision != STATEFUL_ARTIFACT_COMPILER_REVISION_V1
    {
        return Err(StatefulCompilationIdentityErrorV1::BundleMismatch);
    }

    // Touch every untrusted generated field so future format changes cannot accidentally weaken
    // the expectation that all five top-level components are present.
    let _generated_shapes = (
        untrusted.filtered_legacy_ruleset,
        untrusted.stateful_artifact,
        untrusted.union_source_map,
        untrusted.binding,
    );
    let compiled = compile_stateful_spec_bundle_v1(&untrusted.source_spec)
        .map_err(|_| StatefulCompilationIdentityErrorV1::InvalidSource)?;
    let expected = canonical_stateful_bundle_bytes_v1(&compiled)?;
    if expected != bytes {
        return Err(StatefulCompilationIdentityErrorV1::BundleMismatch);
    }
    Ok(compiled)
}

fn bounded_canonical_json<T: Serialize>(
    value: &T,
    max_bytes: usize,
) -> Result<Vec<u8>, StatefulCompilationIdentityErrorV1> {
    let value =
        serde_json::to_value(value).map_err(|_| StatefulCompilationIdentityErrorV1::Encoding)?;
    let bytes = serde_json::to_vec(&canonicalize(value))
        .map_err(|_| StatefulCompilationIdentityErrorV1::Encoding)?;
    if bytes.len() > max_bytes {
        return Err(StatefulCompilationIdentityErrorV1::BoundExceeded);
    }
    Ok(bytes)
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        value => value,
    }
}

fn framed_sha256(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
    digest.finalize().into()
}
