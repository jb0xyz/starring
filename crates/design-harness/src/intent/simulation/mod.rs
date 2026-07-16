mod oracle;
mod support;
#[cfg(test)]
mod tests;
mod trace;

use std::collections::BTreeMap;

use automation_core::{MockInstanceTeardownService, MockMutationAdapter, RunningRuleSetIdentity};
use automation_instance::{
    InMemoryInstanceStore, InstanceId, InstanceRuleSetVersion, SequenceInstanceIdGenerator,
};
use resource_resolution::ResourceBindingMap;
use schemars::JsonSchema;
use serde::Serialize;

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::compile::{compile_intent, CompiledIntentV2};
use super::model::{
    ResolvedCloseControlV1, ResolvedFeatureConfigurationV1, ResolvedManagedPrivateRoomV1,
    PRIVATE_STUDY_ROOM_RECIPE_ID, PRIVATE_STUDY_ROOM_RECIPE_VERSION,
};
use super::normalize::ValidatedIntentV2;
use oracle::prove_close_disabled;
use support::{ensure_hub_binding, identity_error, simulation_error, INSTANCE_ID};
use trace::{run_close_trace, run_help_trace, run_join_trace, run_open_trace, run_submit_trace};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentSimulationReportV1 {
    pub(crate) traces_run: u32,
    pub(crate) close_executed: bool,
}

struct RecipeKeys {
    create_button: String,
    modal: String,
    open_rule: String,
    submit_rule: String,
    member_role: String,
    room_channel: String,
    welcome_panel: String,
    hub_panel: String,
    instance: String,
    help_button: String,
    help_rule: String,
    join_action: String,
    join_rule: String,
    close: Option<CloseKeys>,
}

struct CloseKeys {
    action: String,
    rule: String,
}

struct SimulationRuntime<'a> {
    candidate: &'a Draft,
    bindings: &'a ResourceBindingMap,
    identity: &'a RunningRuleSetIdentity,
    mutation: &'a MockMutationAdapter,
    instances: &'a InMemoryInstanceStore,
    instance_ids: &'a SequenceInstanceIdGenerator,
    teardown: &'a MockInstanceTeardownService,
}

pub(crate) async fn simulate_compiled_intent(
    candidate: &mut Draft,
    intent: &ValidatedIntentV2,
    compiled: &CompiledIntentV2,
    bindings: &ResourceBindingMap,
) -> Result<IntentSimulationReportV1, StructuredError> {
    candidate.simulated_revision = None;
    let result = simulate_private_study_room(candidate, intent, compiled, bindings).await;
    if result.is_ok() {
        candidate.simulated_revision = Some(candidate.draft_revision);
    }
    result
}

async fn simulate_private_study_room(
    candidate: &Draft,
    intent: &ValidatedIntentV2,
    compiled: &CompiledIntentV2,
    bindings: &ResourceBindingMap,
) -> Result<IntentSimulationReportV1, StructuredError> {
    if candidate.validated_revision != Some(candidate.draft_revision) {
        return Err(simulation_error(
            "INTENT_SIMULATION_DRAFT_NOT_VALIDATED",
            "intent.simulation.validation",
            "The compiled candidate has not passed validation at its current revision",
            "Validate the candidate with the same resource bindings before recipe simulation",
        ));
    }
    let expected_compilation = compile_intent(intent)?;
    if &expected_compilation != compiled {
        return Err(simulation_error(
            "INTENT_SIMULATION_COMPILATION_MISMATCH",
            "intent.simulation.compilation",
            "The compiled plan does not match the normalized intent",
            "Compile the current validated intent immediately before simulation",
        ));
    }
    if compiled.manifest.recipe_id != PRIVATE_STUDY_ROOM_RECIPE_ID
        || compiled.manifest.recipe_version != PRIVATE_STUDY_ROOM_RECIPE_VERSION
    {
        return Err(simulation_error(
            "INTENT_SIMULATION_RECIPE_UNSUPPORTED",
            "intent.simulation.recipe",
            "The compiled recipe is not supported by the deterministic simulator",
            "Register an exact simulator for the compiled recipe id and version",
        ));
    }
    let resolved = intent.resolved();
    let [feature] = resolved.features.as_slice() else {
        return Err(simulation_error(
            "INTENT_SIMULATION_FEATURE_CARDINALITY_INVALID",
            "intent.simulation.feature",
            "Recipe simulation requires exactly one normalized feature",
            "Resolve one supported feature before compiling and simulating",
        ));
    };
    if feature.feature_id.as_str() != compiled.manifest.feature_id
        || feature.recipe.id != compiled.manifest.recipe_id
        || feature.recipe.version != compiled.manifest.recipe_version
    {
        return Err(simulation_error(
            "INTENT_SIMULATION_MANIFEST_MISMATCH",
            "intent.simulation.manifest",
            "The compilation manifest does not identify the normalized feature",
            "Discard the stale compiled artifact and compile the current intent",
        ));
    }
    let ResolvedFeatureConfigurationV1::ManagedPrivateRoom(room) = &feature.configuration;
    let expected_bindings = [room.hub_channel.value.as_str().to_string()];
    if compiled.manifest.external_channel_bindings.as_slice() != expected_bindings.as_slice() {
        return Err(simulation_error(
            "INTENT_SIMULATION_BINDING_MANIFEST_MISMATCH",
            "intent.simulation.bindings",
            "The compiled external channel bindings do not match the normalized intent",
            "Compile the current hub channel into the recipe manifest",
        ));
    }
    ensure_hub_binding(room, bindings)?;
    let keys = recipe_keys(compiled, room)?;
    let identity = RunningRuleSetIdentity {
        key: format!("intent-simulation:{}", compiled.manifest.feature_id),
        version: InstanceRuleSetVersion::new(1).map_err(|_| identity_error())?,
    };
    let instance_id = InstanceId::parse(INSTANCE_ID).map_err(|_| identity_error())?;
    let mutation = MockMutationAdapter::new();
    let instances = InMemoryInstanceStore::new();
    let instance_ids = SequenceInstanceIdGenerator::new("study", 1);
    let teardown = MockInstanceTeardownService::new();
    let runtime = SimulationRuntime {
        candidate,
        bindings,
        identity: &identity,
        mutation: &mutation,
        instances: &instances,
        instance_ids: &instance_ids,
        teardown: &teardown,
    };

    run_open_trace(&runtime, &keys).await?;
    let instance = run_submit_trace(&runtime, room, &keys, &instance_id).await?;
    run_help_trace(&runtime, room, &keys).await?;
    run_join_trace(&runtime, room, &keys, &instance).await?;
    let close_executed = match (&room.controls.close, &keys.close) {
        (ResolvedCloseControlV1::Disabled { .. }, None) => {
            prove_close_disabled(candidate, compiled)?;
            false
        }
        (ResolvedCloseControlV1::AnyMember { response, .. }, Some(close)) => {
            run_close_trace(&runtime, response.value.as_str(), close, &instance).await?;
            true
        }
        _ => {
            return Err(simulation_error(
                "INTENT_SIMULATION_CLOSE_POLICY_MISMATCH",
                "intent.simulation.close",
                "The compiled close controls do not match the normalized close policy",
                "Recompile the current normalized intent before simulation",
            ));
        }
    };
    Ok(IntentSimulationReportV1 {
        traces_run: if close_executed { 5 } else { 4 },
        close_executed,
    })
}

fn recipe_keys(
    compiled: &CompiledIntentV2,
    room: &ResolvedManagedPrivateRoomV1,
) -> Result<RecipeKeys, StructuredError> {
    let generated = &compiled.manifest.generated_objects;
    let close = match &room.controls.close {
        ResolvedCloseControlV1::Disabled { .. } => {
            if generated.contains_key("close") || generated.contains_key("close_room") {
                return Err(manifest_object_error("close"));
            }
            None
        }
        ResolvedCloseControlV1::AnyMember { .. } => Some(CloseKeys {
            action: generated_object(generated, "close")?,
            rule: generated_object(generated, "close_room")?,
        }),
    };
    Ok(RecipeKeys {
        create_button: generated_object(generated, "create_study_room")?,
        modal: generated_object(generated, "study_modal")?,
        open_rule: generated_object(generated, "open_modal")?,
        submit_rule: generated_object(generated, "submit_room")?,
        member_role: generated_object(generated, "member_role")?,
        room_channel: generated_object(generated, "room_channel")?,
        welcome_panel: generated_object(generated, "welcome_panel")?,
        hub_panel: generated_object(generated, "hub_panel")?,
        instance: generated_object(generated, "study_instance")?,
        help_button: generated_object(generated, "study_help")?,
        help_rule: generated_object(generated, "show_help")?,
        join_action: generated_object(generated, "join")?,
        join_rule: generated_object(generated, "join_room")?,
        close,
    })
}

fn generated_object(
    generated: &BTreeMap<String, String>,
    symbol: &str,
) -> Result<String, StructuredError> {
    generated
        .get(symbol)
        .cloned()
        .ok_or_else(|| manifest_object_error(symbol))
}

fn manifest_object_error(symbol: &str) -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_MANIFEST_OBJECT_MISSING",
        format!("intent.simulation.manifest.generated_objects.{symbol}"),
        format!("The recipe manifest does not contain generated object {symbol}"),
        "Compile a complete deterministic recipe manifest before simulation",
    )
}
