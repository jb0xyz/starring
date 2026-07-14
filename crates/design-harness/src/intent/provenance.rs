use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IntentCoverageV1 {
    pub clause: String,
    pub requirement_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementProvenanceV1 {
    pub requirement_id: String,
    pub feature_id: String,
    pub recipe_id: String,
    pub recipe_version: u32,
    pub clause: String,
    pub intent_paths: Vec<String>,
}

pub(super) struct ProvenanceBuilder {
    feature_id: String,
    recipe_id: String,
    recipe_version: u32,
    coverage: BTreeMap<String, Vec<String>>,
    requirements: Vec<RequirementProvenanceV1>,
}

impl ProvenanceBuilder {
    pub(super) fn new(feature_id: &str, recipe_id: &str, recipe_version: u32) -> Self {
        Self {
            feature_id: feature_id.to_string(),
            recipe_id: recipe_id.to_string(),
            recipe_version,
            coverage: BTreeMap::new(),
            requirements: Vec::new(),
        }
    }

    pub(super) fn record(&mut self, requirement_id: &str, clause: &str, intent_paths: &[&str]) {
        self.coverage
            .entry(clause.to_string())
            .or_default()
            .push(requirement_id.to_string());
        self.requirements.push(RequirementProvenanceV1 {
            requirement_id: requirement_id.to_string(),
            feature_id: self.feature_id.clone(),
            recipe_id: self.recipe_id.clone(),
            recipe_version: self.recipe_version,
            clause: clause.to_string(),
            intent_paths: intent_paths
                .iter()
                .map(|path| (*path).to_string())
                .collect(),
        });
    }

    pub(super) fn finish(self) -> (Vec<IntentCoverageV1>, Vec<RequirementProvenanceV1>) {
        let coverage = self
            .coverage
            .into_iter()
            .map(|(clause, requirement_ids)| IntentCoverageV1 {
                clause,
                requirement_ids,
            })
            .collect();
        (coverage, self.requirements)
    }
}
