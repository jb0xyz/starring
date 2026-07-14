use serde_json::Value;

use crate::turn::IntentRecipeDetailFacetV3;

use super::evidence::IntentRecipeEvidenceV3;

const CORE_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DETAIL_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn default_and_model_detail_evidence_validate_under_the_closed_registry() {
    let default = IntentRecipeEvidenceV3::deterministic_default(CORE_DIGEST).unwrap();
    default.validate().unwrap();
    let default_json = serde_json::to_value(&default).unwrap();
    assert_eq!(default_json["extraction_mode"], "deterministic_default");
    assert_eq!(default_json["detail_facets"], serde_json::json!([]));
    assert_eq!(default_json["detail_result_digest"], Value::Null);

    let detailed = IntentRecipeEvidenceV3::model_detail(
        CORE_DIGEST,
        &[
            IntentRecipeDetailFacetV3::Naming,
            IntentRecipeDetailFacetV3::Copy,
        ],
        DETAIL_DIGEST.to_string(),
    )
    .unwrap();
    detailed.validate().unwrap();
    let detailed_json = serde_json::to_value(&detailed).unwrap();
    assert_eq!(detailed_json["extraction_mode"], "model_detail");
    assert_eq!(
        detailed_json["detail_facets"],
        serde_json::json!(["copy", "naming"])
    );
    assert_eq!(detailed_json["detail_result_digest"], DETAIL_DIGEST);
    assert_ne!(
        default_json["detail_request_digest"],
        detailed_json["detail_request_digest"]
    );
}

#[test]
fn recipe_evidence_rejects_tampering_and_invalid_mode_shapes() {
    let evidence = IntentRecipeEvidenceV3::model_detail(
        CORE_DIGEST,
        &[IntentRecipeDetailFacetV3::Controls],
        DETAIL_DIGEST.to_string(),
    )
    .unwrap();
    for field in [
        "core_semantic_digest",
        "recipe_id",
        "recipe_version",
        "registry_digest",
        "selected_descriptor_digest",
        "detail_request_digest",
        "detail_result_digest",
        "detail_coverage_digest",
    ] {
        let mut value = serde_json::to_value(&evidence).unwrap();
        value[field] = match field {
            "recipe_version" => serde_json::json!(99),
            _ => serde_json::json!(
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
            ),
        };
        let tampered: IntentRecipeEvidenceV3 = serde_json::from_value(value).unwrap();
        assert_eq!(
            tampered.validate().unwrap_err().code,
            "INVALID_INTENT_RECIPE_EVIDENCE"
        );
    }

    let mut value = serde_json::to_value(&evidence).unwrap();
    value["extraction_mode"] = serde_json::json!("deterministic_default");
    let tampered: IntentRecipeEvidenceV3 = serde_json::from_value(value).unwrap();
    assert_eq!(
        tampered.validate().unwrap_err().code,
        "INVALID_INTENT_RECIPE_EVIDENCE"
    );
}

#[test]
fn recipe_evidence_rejects_duplicate_facets_before_hashing() {
    assert_eq!(
        IntentRecipeEvidenceV3::model_detail(
            CORE_DIGEST,
            &[
                IntentRecipeDetailFacetV3::Copy,
                IntentRecipeDetailFacetV3::Copy,
            ],
            DETAIL_DIGEST.to_string(),
        )
        .unwrap_err()
        .code,
        "DUPLICATE_RECIPE_DETAIL_FACET"
    );
}
