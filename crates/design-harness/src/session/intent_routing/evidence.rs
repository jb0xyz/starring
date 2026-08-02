use serde::{Deserialize, Serialize};

use crate::errors::StructuredError;
use crate::intent::identity::{canonical_json_digest, is_lowercase_sha256_hex, IdentityErrorSpec};
use crate::intent::{
    recipe_descriptor_digest_v1, recipe_registry_digest_v1, RecipeKindV1,
    PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use crate::turn::IntentRecipeDetailFacetV3;

use super::state::intent_error;

const DETAIL_REQUEST_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.detail_request.v2\0";
const DETAIL_COVERAGE_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.detail_coverage.v2\0";
const RECIPE_EVIDENCE_BINDING_DIGEST_DOMAIN_V4: &[u8] =
    b"starring.intent.recipe_evidence_binding.v4\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntentRecipeExtractionModeV4 {
    DeterministicDefault,
    ModelDetail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentRecipeEvidenceV4 {
    core_semantic_digest: String,
    source_human_turn_digest: String,
    recipe_id: String,
    recipe_version: u32,
    registry_digest: String,
    selected_descriptor_digest: String,
    extraction_mode: IntentRecipeExtractionModeV4,
    detail_facets: Vec<IntentRecipeDetailFacetV3>,
    detail_request_digest: String,
    detail_result_digest: Option<String>,
    detail_coverage_digest: String,
}

impl IntentRecipeEvidenceV4 {
    pub(super) fn deterministic_default(
        core_semantic_digest: &str,
        source_human_turn_digest: &str,
    ) -> Result<Self, StructuredError> {
        Self::build(
            core_semantic_digest,
            source_human_turn_digest,
            IntentRecipeExtractionModeV4::DeterministicDefault,
            &[],
            None,
        )
    }

    pub(super) fn model_detail(
        core_semantic_digest: &str,
        source_human_turn_digest: &str,
        detail_facets: &[IntentRecipeDetailFacetV3],
        detail_result_digest: String,
    ) -> Result<Self, StructuredError> {
        Self::build(
            core_semantic_digest,
            source_human_turn_digest,
            IntentRecipeExtractionModeV4::ModelDetail,
            detail_facets,
            Some(detail_result_digest),
        )
    }

    pub(super) fn core_semantic_digest(&self) -> &str {
        &self.core_semantic_digest
    }

    pub(super) fn source_human_turn_digest(&self) -> &str {
        &self.source_human_turn_digest
    }

    pub(super) fn recipe_id(&self) -> &str {
        &self.recipe_id
    }

    pub(super) fn recipe_version(&self) -> u32 {
        self.recipe_version
    }

    pub(super) fn registry_digest(&self) -> &str {
        &self.registry_digest
    }

    pub(super) fn selected_descriptor_digest(&self) -> &str {
        &self.selected_descriptor_digest
    }

    pub(super) fn binding_digest(&self) -> Result<String, StructuredError> {
        self.validate()?;
        canonical_json_digest(
            RECIPE_EVIDENCE_BINDING_DIGEST_DOMAIN_V4,
            self,
            IdentityErrorSpec::new(
                "INTENT_RECIPE_EVIDENCE_BINDING_SERIALIZATION_FAILED",
                "intent.recipe_evidence",
                "The recipe evidence binding could not be serialized deterministically",
            ),
        )
    }

    pub(super) fn validate(&self) -> Result<(), StructuredError> {
        let canonical_facets = canonical_facets(&self.detail_facets)?;
        let registry_digest = recipe_registry_digest_v1()?;
        let selected_descriptor_digest =
            recipe_descriptor_digest_v1(RecipeKindV1::PrivateStudyRoomV1)?;
        let expected_request_digest = detail_request_digest(
            &self.core_semantic_digest,
            &self.source_human_turn_digest,
            &self.recipe_id,
            self.recipe_version,
            &canonical_facets,
        )?;
        let expected_coverage_digest = detail_coverage_digest(
            &expected_request_digest,
            self.detail_result_digest.as_deref(),
            &canonical_facets,
        )?;
        let valid_mode = match self.extraction_mode {
            IntentRecipeExtractionModeV4::DeterministicDefault => {
                canonical_facets.is_empty() && self.detail_result_digest.is_none()
            }
            IntentRecipeExtractionModeV4::ModelDetail => {
                !canonical_facets.is_empty()
                    && self.detail_result_digest.as_deref().is_some_and(valid_hash)
            }
        };
        if self.recipe_id != PRIVATE_STUDY_ROOM_RECIPE_ID
            || self.recipe_version != PRIVATE_STUDY_ROOM_RECIPE_VERSION
            || self.registry_digest != registry_digest
            || self.selected_descriptor_digest != selected_descriptor_digest
            || self.detail_facets != canonical_facets
            || self.detail_request_digest != expected_request_digest
            || self.detail_coverage_digest != expected_coverage_digest
            || !valid_hash(&self.core_semantic_digest)
            || !valid_hash(&self.source_human_turn_digest)
            || !valid_mode
        {
            return Err(intent_error(
                "INVALID_INTENT_RECIPE_EVIDENCE",
                "intent.recipe_evidence",
                "The recipe evidence does not match the current Core, registry, or extraction contract",
                "Restart the intent under the current protocol and closed recipe registry",
            ));
        }
        Ok(())
    }

    fn build(
        core_semantic_digest: &str,
        source_human_turn_digest: &str,
        extraction_mode: IntentRecipeExtractionModeV4,
        detail_facets: &[IntentRecipeDetailFacetV3],
        detail_result_digest: Option<String>,
    ) -> Result<Self, StructuredError> {
        let detail_facets = canonical_facets(detail_facets)?;
        let recipe_id = PRIVATE_STUDY_ROOM_RECIPE_ID.to_string();
        let recipe_version = PRIVATE_STUDY_ROOM_RECIPE_VERSION;
        let registry_digest = recipe_registry_digest_v1()?;
        let selected_descriptor_digest =
            recipe_descriptor_digest_v1(RecipeKindV1::PrivateStudyRoomV1)?;
        let detail_request_digest = detail_request_digest(
            core_semantic_digest,
            source_human_turn_digest,
            &recipe_id,
            recipe_version,
            &detail_facets,
        )?;
        let detail_coverage_digest = detail_coverage_digest(
            &detail_request_digest,
            detail_result_digest.as_deref(),
            &detail_facets,
        )?;
        let evidence = Self {
            core_semantic_digest: core_semantic_digest.to_string(),
            source_human_turn_digest: source_human_turn_digest.to_string(),
            recipe_id,
            recipe_version,
            registry_digest,
            selected_descriptor_digest,
            extraction_mode,
            detail_facets,
            detail_request_digest,
            detail_result_digest,
            detail_coverage_digest,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

fn canonical_facets(
    values: &[IntentRecipeDetailFacetV3],
) -> Result<Vec<IntentRecipeDetailFacetV3>, StructuredError> {
    if values.len() > 3 {
        return Err(intent_error(
            "TOO_MANY_RECIPE_DETAIL_FACETS",
            "intent.recipe_evidence.detail_facets",
            "Recipe evidence contains more than three detail facets",
            "Use only copy, naming, and controls",
        ));
    }
    let mut facets = values.to_vec();
    facets.sort();
    facets.dedup();
    if facets.len() != values.len() {
        return Err(intent_error(
            "DUPLICATE_RECIPE_DETAIL_FACET",
            "intent.recipe_evidence.detail_facets",
            "Recipe evidence contains a duplicate detail facet",
            "Bind each selected facet exactly once",
        ));
    }
    Ok(facets)
}

#[derive(Serialize)]
struct DetailRequestProjectionV2<'a> {
    core_semantic_digest: &'a str,
    source_human_turn_digest: &'a str,
    recipe_id: &'a str,
    recipe_version: u32,
    detail_facets: &'a [IntentRecipeDetailFacetV3],
}

#[derive(Serialize)]
struct DetailCoverageProjectionV2<'a> {
    detail_request_digest: &'a str,
    detail_result_digest: Option<&'a str>,
    covered_facets: &'a [IntentRecipeDetailFacetV3],
}

fn detail_request_digest(
    core_semantic_digest: &str,
    source_human_turn_digest: &str,
    recipe_id: &str,
    recipe_version: u32,
    detail_facets: &[IntentRecipeDetailFacetV3],
) -> Result<String, StructuredError> {
    stable_hash(
        DETAIL_REQUEST_DIGEST_DOMAIN_V2,
        &DetailRequestProjectionV2 {
            core_semantic_digest,
            source_human_turn_digest,
            recipe_id,
            recipe_version,
            detail_facets,
        },
        "intent.recipe_evidence.detail_request_digest",
    )
}

fn detail_coverage_digest(
    detail_request_digest: &str,
    detail_result_digest: Option<&str>,
    covered_facets: &[IntentRecipeDetailFacetV3],
) -> Result<String, StructuredError> {
    stable_hash(
        DETAIL_COVERAGE_DIGEST_DOMAIN_V2,
        &DetailCoverageProjectionV2 {
            detail_request_digest,
            detail_result_digest,
            covered_facets,
        },
        "intent.recipe_evidence.detail_coverage_digest",
    )
}

fn stable_hash(
    domain: &[u8],
    value: &impl Serialize,
    location: &str,
) -> Result<String, StructuredError> {
    canonical_json_digest(
        domain,
        value,
        IdentityErrorSpec::new(
            "INTENT_RECIPE_EVIDENCE_SERIALIZATION_FAILED",
            location,
            "Recipe evidence could not be serialized deterministically",
        ),
    )
}

fn valid_hash(value: &str) -> bool {
    is_lowercase_sha256_hex(value)
}
