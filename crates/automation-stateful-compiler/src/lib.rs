//! Pure, immutable StatefulSpec R0 compilation artifacts.
//!
//! The compiler emits two deliberately separate targets: a legacy RuleSet containing assets and
//! unconditional stateless workflows only, and an opaque stateful artifact. It never represents a
//! stateful workflow as a legacy rule. The resulting bundle is identity-bound but remains
//! non-deployable; no database, activation, dispatcher, or live runtime integration lives here.

mod canonical;
mod compatibility;
mod compile;
mod digest;
mod model;

pub use canonical::{
    canonical_compiled_state_schema_bytes_v1, canonical_stateful_artifact_bytes_v1,
    canonical_stateful_bundle_bytes_v1, canonical_stateful_compilation_binding_bytes_v1,
    canonical_stateful_union_source_map_bytes_v1, decode_canonical_stateful_bundle_v1,
    stateful_artifact_digest_v1, stateful_bundle_digest_v1, stateful_compilation_binding_digest_v1,
    stateful_state_schema_digest_v1, stateful_union_source_map_digest_v1,
    StatefulCompilationIdentityErrorV1, MAX_COMPILED_STATE_SCHEMA_CANONICAL_BYTES_V1,
    MAX_STATEFUL_ARTIFACT_CANONICAL_BYTES_V1, MAX_STATEFUL_BUNDLE_CANONICAL_BYTES_V1,
    MAX_STATEFUL_COMPILATION_BINDING_CANONICAL_BYTES_V1,
    MAX_STATEFUL_UNION_SOURCE_MAP_CANONICAL_BYTES_V1,
};
pub use compatibility::{
    check_additive_state_schema_compatibility_v1, AdditiveStateSchemaCompatibilityV1,
    StateSchemaCompatibilityErrorV1,
};
pub use compile::{compile_stateful_spec_bundle_v1, StatefulSpecCompileErrorV1};
pub use digest::{
    StateDeclarationDigestV1, StatefulArtifactDigestV1, StatefulBundleDigestV1,
    StatefulCompilationBindingDigestV1, StatefulStateSchemaDigestV1,
    StatefulUnionSourceMapDigestV1,
};
pub use model::{
    CompiledAcknowledgementStrategyV1, CompiledStateSchemaV1, CompiledStateVariableV1,
    CompiledStatefulArtifactV1, CompiledStatefulBranchV1, CompiledStatefulBundleV1,
    CompiledStatefulWorkflowV1, CompiledWorkflowDependenciesV1, StatefulArtifactIdentityV1,
    StatefulBranchSourceMapV1, StatefulCompilationBindingV1, StatefulNodeSourceMapV1,
    StatefulResponseSourceMapV1, StatefulSourceSpecIdentityV1, StatefulStateSchemaIdentityV1,
    StatefulStateVariableSourceMapV1, StatefulStatelessWorkflowSourceMapV1,
    StatefulUnionSourceMapIdentityV1, StatefulUnionSourceMapV1, StatefulWorkflowSourceMapV1,
    STATEFUL_ARTIFACT_COMPILER_REVISION_V1, STATEFUL_ARTIFACT_KIND_V1,
    STATEFUL_ARTIFACT_SCHEMA_VERSION_V1, STATEFUL_BUNDLE_FORMAT_VERSION_V1,
    STATEFUL_BUNDLE_KIND_V1, STATEFUL_COMPILATION_BINDING_FORMAT_VERSION_V1,
    STATEFUL_COMPILATION_BINDING_KIND_V1, STATEFUL_STATE_SCHEMA_KIND_V1,
    STATEFUL_STATE_SCHEMA_VERSION_V1, STATEFUL_UNION_SOURCE_MAP_KIND_V1,
    STATEFUL_UNION_SOURCE_MAP_SCHEMA_VERSION_V1,
};

/// This milestone creates verifiable artifacts only. Live activation remains intentionally absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("StatefulSpec compiled bundles are not deployable in R0")]
pub struct StatefulCompiledBundleDeploymentUnavailableV1;
