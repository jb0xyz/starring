use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::Serialize;

use crate::errors::StructuredError;
use crate::turn::{
    ScopeAction, ScopeButtonRoute, ScopePostPanelButtonRoute, ScopeRequirement, ScopeTrigger,
};

use super::catalog::{
    recipe_registry_digest_v1, registered_recipe_kind_v1, RecipeDescriptorV1, RecipeKindV1,
    MAX_COMPILED_REQUIREMENTS,
};
use super::identity::{canonical_json_digest, IdentityErrorSpec};
use super::model::{IntentRequestedOutcome, ResolvedFeatureConfigurationV1};
use super::normalize::ValidatedIntentV2;
use super::private_study_room::compile_private_study_room;
use super::provenance::{IntentCoverageV1, ProvenanceBuilder, RequirementProvenanceV1};
use super::semantic::semantic_intent_hash;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompiledIntentV2 {
    pub requirements: Vec<ScopeRequirement>,
    pub coverage: Vec<IntentCoverageV1>,
    pub requirement_provenance: Vec<RequirementProvenanceV1>,
    pub manifest: CompilationManifestV2,
    pub verification: CompilationVerificationV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompilationManifestV2 {
    pub identity_revision: u16,
    pub compiler_revision: u32,
    pub registry_digest: String,
    pub compiler_input_hash: String,
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

pub const INTENT_IDENTITY_REVISION: u16 = 2;

const COMPILER_INPUT_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.compiler_input.v2\0";
const COMPILED_PLAN_DIGEST_DOMAIN_V2: &[u8] = b"starring.intent.compiled_plan.v2\0";

pub(super) struct RecipeExpansion {
    pub(super) descriptor: RecipeDescriptorV1,
    pub(super) feature_id: String,
    pub(super) requirements: Vec<ScopeRequirement>,
    pub(super) provenance: ProvenanceBuilder,
    pub(super) generated_objects: BTreeMap<String, String>,
    pub(super) external_channel_bindings: Vec<String>,
}

pub fn compile_intent(intent: &ValidatedIntentV2) -> Result<CompiledIntentV2, StructuredError> {
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
            "Intent V2 compilation requires exactly one normalized feature",
            "Resolve a single-feature Intent V2 before compilation",
        ));
    };
    let recipe_kind = registered_recipe_kind_v1(&feature.recipe.id, feature.recipe.version)?;
    let expansion = match recipe_kind {
        RecipeKindV1::PrivateStudyRoomV1 => {
            let ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) = &feature.configuration;
            compile_private_study_room(feature.feature_id.as_str(), room)
        }
    };
    finalize_compilation(intent, expansion)
}

pub(crate) fn compiled_intents_behaviorally_equivalent(
    left: &CompiledIntentV2,
    right: &CompiledIntentV2,
) -> bool {
    let mut normalized = left.clone();
    normalized.manifest.compiler_input_hash = right.manifest.compiler_input_hash.clone();
    normalized.manifest.semantic_intent_hash = right.manifest.semantic_intent_hash.clone();
    normalized == *right
}

pub(crate) fn verify_outcome_only_finalization(
    working: &CompiledIntentV2,
    standalone: &CompiledIntentV2,
    finalized: &CompiledIntentV2,
) -> Result<(), StructuredError> {
    if compiled_intents_behaviorally_equivalent(working, standalone)
        && compiled_intents_behaviorally_equivalent(working, finalized)
        && standalone.manifest.semantic_intent_hash == finalized.manifest.semantic_intent_hash
    {
        return Ok(());
    }
    Err(compile_error(
        "INTENT_OUTCOME_FINALIZATION_BEHAVIOR_CHANGED",
        "intent.compiler",
        "The finalization request changes semantic, executable, or compiler-owned behavior",
        "Commit behavior changes as a working Draft before requesting validated_preview",
    ))
}

fn finalize_compilation(
    intent: &ValidatedIntentV2,
    expansion: RecipeExpansion,
) -> Result<CompiledIntentV2, StructuredError> {
    let RecipeExpansion {
        descriptor,
        feature_id,
        requirements,
        provenance,
        generated_objects,
        external_channel_bindings,
    } = expansion;
    if requirements.len() < descriptor.min_requirements
        || requirements.len() > descriptor.max_requirements
    {
        return Err(compile_error(
            "INTENT_RECIPE_REQUIREMENT_BOUND_EXCEEDED",
            "intent.compiler.recipe_requirements",
            format!(
                "Recipe {} version {} emitted {} requirements outside its pinned bounds {}..={}",
                descriptor.id,
                descriptor.version,
                requirements.len(),
                descriptor.min_requirements,
                descriptor.max_requirements
            ),
            "Correct the registered compiler or revise the pinned recipe descriptor",
        ));
    }
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
    let registered_kind = registered_recipe_kind_v1(descriptor.id, descriptor.version)?;
    if registered_kind != descriptor.kind {
        return Err(compile_error(
            "INTENT_RECIPE_DESCRIPTOR_UNREGISTERED",
            "intent.compiler.recipe_descriptor",
            "The recipe expansion descriptor does not match the closed registry",
            "Compile through the exact registered recipe kind",
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
    let manifest = CompilationManifestV2 {
        identity_revision: INTENT_IDENTITY_REVISION,
        compiler_revision: descriptor.compiler_revision,
        registry_digest: recipe_registry_digest_v1()?,
        compiler_input_hash: canonical_json_digest(
            COMPILER_INPUT_DIGEST_DOMAIN_V2,
            intent,
            compiler_identity_error("intent.compiler.input"),
        )?,
        semantic_intent_hash: semantic_intent_hash(intent)?,
        compiled_plan_hash: canonical_json_digest(
            COMPILED_PLAN_DIGEST_DOMAIN_V2,
            &requirements,
            compiler_identity_error("intent.compiler.requirements"),
        )?,
        feature_id,
        recipe_id: descriptor.id.to_string(),
        recipe_version: descriptor.version,
        generated_objects,
        external_channel_bindings,
    };
    Ok(CompiledIntentV2 {
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

fn compiler_identity_error(location: &'static str) -> IdentityErrorSpec<'static> {
    IdentityErrorSpec::new(
        "INTENT_COMPILER_SERIALIZATION_FAILED",
        location,
        "A deterministic compiler artifact could not be serialized",
    )
}

fn compile_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
