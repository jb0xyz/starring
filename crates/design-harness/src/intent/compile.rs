use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::errors::StructuredError;
use crate::turn::{
    ScopeAction, ScopeButtonRoute, ScopePostPanelButtonRoute, ScopeRequirement, ScopeTrigger,
};

use super::catalog::{RecipeDescriptor, COMPILER_REVISION, MAX_COMPILED_REQUIREMENTS};
use super::model::{IntentRequestedOutcome, ResolvedFeatureConfigurationV1};
use super::normalize::ValidatedIntentV1;
use super::private_study_room::compile_private_study_room;
use super::provenance::{IntentCoverageV1, ProvenanceBuilder, RequirementProvenanceV1};
use super::semantic::semantic_intent_hash;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompiledIntentV1 {
    pub requirements: Vec<ScopeRequirement>,
    pub coverage: Vec<IntentCoverageV1>,
    pub requirement_provenance: Vec<RequirementProvenanceV1>,
    pub manifest: CompilationManifestV1,
    pub verification: CompilationVerificationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompilationManifestV1 {
    pub compiler_revision: u32,
    pub registry_digest: String,
    pub input_intent_hash: String,
    pub semantic_intent_hash: String,
    pub compiled_plan_hash: String,
    pub feature_id: String,
    pub recipe_id: String,
    pub recipe_version: u32,
    pub generated_objects: BTreeMap<String, String>,
    pub external_channel_bindings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompilationVerificationV1 {
    pub actionable_requirements: usize,
    pub covered_requirements: usize,
    pub rendered_buttons: usize,
    pub matched_button_handlers: usize,
}

pub(super) struct RecipeExpansion {
    pub(super) descriptor: RecipeDescriptor,
    pub(super) feature_id: String,
    pub(super) requirements: Vec<ScopeRequirement>,
    pub(super) provenance: ProvenanceBuilder,
    pub(super) generated_objects: BTreeMap<String, String>,
    pub(super) external_channel_bindings: Vec<String>,
}

pub fn compile_intent(intent: &ValidatedIntentV1) -> Result<CompiledIntentV1, StructuredError> {
    let resolved = intent.resolved();
    if resolved.requested_outcome == IntentRequestedOutcome::Discussion {
        return Err(compile_error(
            "INTENT_OUTCOME_NOT_COMPILABLE",
            "intent.requested_outcome",
            "A discussion intent cannot produce Draft mutations",
            "Change the requested outcome to working_draft or validated_preview after the user asks to build",
        ));
    }
    let [feature] = resolved.features.as_slice() else {
        return Err(compile_error(
            "INTENT_FEATURE_CARDINALITY_INVALID",
            "intent.features",
            "Intent V1 compilation requires exactly one normalized feature",
            "Resolve a single-feature Intent V1 before compilation",
        ));
    };
    let ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) = &feature.configuration;
    finalize_compilation(
        intent,
        compile_private_study_room(feature.feature_id.as_str(), room),
    )
}

fn finalize_compilation(
    intent: &ValidatedIntentV1,
    expansion: RecipeExpansion,
) -> Result<CompiledIntentV1, StructuredError> {
    let RecipeExpansion {
        descriptor,
        feature_id,
        requirements,
        provenance,
        generated_objects,
        external_channel_bindings,
    } = expansion;
    if requirements.len() > MAX_COMPILED_REQUIREMENTS {
        return Err(compile_error(
            "COMPILED_INTENT_TOO_LARGE",
            "intent.compiler.requirements",
            format!(
                "The recipe emitted {} requirements but the maximum before the final guard is {MAX_COMPILED_REQUIREMENTS}",
                requirements.len()
            ),
            "Reduce the recipe footprint or split it into bounded transactions",
        ));
    }
    let verification = verify_compilation(&requirements)?;
    let (coverage, requirement_provenance) = provenance.finish();
    if verification.covered_requirements != requirement_provenance.len() {
        return Err(compile_error(
            "INTENT_PROVENANCE_INCOMPLETE",
            "intent.compiler.provenance",
            "Not every compiled requirement has provenance",
            "Map every generated requirement to exactly one high-level intent clause",
        ));
    }
    let manifest = CompilationManifestV1 {
        compiler_revision: COMPILER_REVISION,
        registry_digest: stable_hash(&descriptor, "intent.compiler.registry")?,
        input_intent_hash: stable_hash(intent, "intent.compiler.input")?,
        semantic_intent_hash: semantic_intent_hash(intent)?,
        compiled_plan_hash: stable_hash(&requirements, "intent.compiler.requirements")?,
        feature_id,
        recipe_id: descriptor.id.to_string(),
        recipe_version: descriptor.version,
        generated_objects,
        external_channel_bindings,
    };
    Ok(CompiledIntentV1 {
        requirements,
        coverage,
        requirement_provenance,
        manifest,
        verification,
    })
}

fn verify_compilation(
    requirements: &[ScopeRequirement],
) -> Result<CompilationVerificationV1, StructuredError> {
    let mut ids = BTreeSet::new();
    let mut routes = Vec::new();
    let mut handlers = BTreeMap::<String, usize>::new();
    for requirement in requirements {
        if !ids.insert(requirement.id()) {
            return Err(compile_error(
                "DUPLICATE_COMPILED_REQUIREMENT_ID",
                "intent.compiler.requirements",
                format!("Requirement id {} is repeated", requirement.id()),
                "Use one stable semantic ID per generated requirement",
            ));
        }
        match requirement {
            ScopeRequirement::Button { route, .. } => match route {
                ScopeButtonRoute::Static { key } => routes.push(format!("static:{key}")),
                ScopeButtonRoute::InstanceAction { action } => {
                    routes.push(format!("instance:{action}"));
                }
            },
            ScopeRequirement::Rule { trigger, .. } => {
                let route = match trigger {
                    ScopeTrigger::ButtonClick { component } => format!("static:{component}"),
                    ScopeTrigger::InstanceAction { action } => format!("instance:{action}"),
                    ScopeTrigger::ModalSubmit { .. } => continue,
                };
                *handlers.entry(route).or_default() += 1;
            }
            ScopeRequirement::Action {
                action: ScopeAction::PostPanel { buttons, .. },
                ..
            } => {
                for button in buttons {
                    routes.push(match &button.route {
                        ScopePostPanelButtonRoute::Static { key } => format!("static:{key}"),
                        ScopePostPanelButtonRoute::InstanceAction { action, .. } => {
                            format!("instance:{action}")
                        }
                    });
                }
            }
            _ => {}
        }
    }
    for route in &routes {
        if handlers.get(route) != Some(&1) {
            return Err(compile_error(
                "COMPILED_INTENT_DEAD_BUTTON",
                "intent.compiler.buttons",
                format!("Rendered route {route} does not have exactly one handler"),
                "Compile exactly one matching rule for every rendered button route",
            ));
        }
    }
    Ok(CompilationVerificationV1 {
        actionable_requirements: requirements.len(),
        covered_requirements: requirements.len(),
        rendered_buttons: routes.len(),
        matched_button_handlers: routes.len(),
    })
}

fn stable_hash(value: &impl Serialize, location: &str) -> Result<String, StructuredError> {
    let bytes = serde_json::to_vec(value).map_err(|error| {
        compile_error(
            "INTENT_COMPILER_SERIALIZATION_FAILED",
            location,
            "A deterministic compiler artifact could not be serialized",
            error.to_string(),
        )
    })?;
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    Ok(output)
}

fn compile_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
