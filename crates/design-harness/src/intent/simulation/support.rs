use std::collections::BTreeMap;

use automation_core::{SanitizeContext, TemplateString};
use resource_resolution::ResourceBindingMap;

use crate::errors::{translate_run_error, StructuredError};

use super::super::model::{ResolvedManagedPrivateRoomV1, RoomNamePatternV1};

pub(super) const GUILD_ID: &str = "1";
pub(super) const CREATOR_ID: &str = "42";
pub(super) const JOINER_ID: &str = "77";
pub(super) const CLOSER_ID: &str = "88";
pub(super) const ROOM_NAME_INPUT: &str = "room_name";
pub(super) const ROOM_NAME_VALUE: &str = "algebra";
pub(super) const INSTANCE_ID: &str = "study_001";
pub(super) const MEMBER_ROLE_ALIAS: &str = "member_role";
pub(super) const ROOM_CHANNEL_ALIAS: &str = "room_channel";
pub(super) const WELCOME_PANEL_ALIAS: &str = "welcome_panel";
pub(super) const HUB_PANEL_ALIAS: &str = "hub_panel";
pub(super) const VIEW_CHANNEL_BIT: u64 = 1 << 10;

pub(super) fn ensure_hub_binding(
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
) -> Result<(), StructuredError> {
    bound_hub_channel(room, bindings).map(|_| ())
}

pub(super) fn bound_hub_channel(
    room: &ResolvedManagedPrivateRoomV1,
    bindings: &ResourceBindingMap,
) -> Result<String, StructuredError> {
    bindings
        .channel_bindings
        .iter()
        .find_map(|(key, channel)| {
            (key.0 == room.hub_channel.value.as_str()).then(|| channel.to_string())
        })
        .ok_or_else(|| {
            simulation_error(
                "INTENT_SIMULATION_BINDING_MISSING",
                "intent.simulation.bindings.hub_channel",
                format!(
                    "Hub channel binding {} is missing",
                    room.hub_channel.value.as_str()
                ),
                "Provide the exact external channel binding recorded by the normalized intent",
            )
        })
}

pub(super) fn render_pattern(
    pattern: &RoomNamePatternV1,
    inputs: &BTreeMap<String, String>,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    render_source(
        format!(
            "{}${{input.{ROOM_NAME_INPUT}}}{}",
            pattern.prefix, pattern.suffix
        )
        .as_str(),
        inputs,
        context,
        location,
    )
}

pub(super) fn render_literal(
    source: &str,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    render_source(source, &BTreeMap::new(), context, location)
}

fn render_source(
    source: &str,
    inputs: &BTreeMap<String, String>,
    context: SanitizeContext,
    location: &str,
) -> Result<String, StructuredError> {
    TemplateString::parse(source)
        .and_then(|template| template.render(inputs, context))
        .map_err(|_| {
            simulation_error(
                "INTENT_SIMULATION_ORACLE_RENDER_FAILED",
                format!("intent.simulation.{location}"),
                "Normalized recipe copy could not be rendered by the runtime template engine",
                "Reject invalid room-name input or normalize copy within runtime limits",
            )
        })
}

pub(super) fn runtime_error(trace: &str, error: &automation_core::AdapterError) -> StructuredError {
    let translated = translate_run_error(error);
    StructuredError::new(
        translated.code,
        format!("intent.simulation.{trace}"),
        translated.message,
        translated.hint,
    )
}

pub(super) fn trace_error(trace: &str, message: &str, hint: &str) -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_TRACE_MISMATCH",
        format!("intent.simulation.{trace}"),
        message,
        hint,
    )
}

pub(super) fn instance_manifest_error() -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_INSTANCE_MANIFEST_INVALID",
        "intent.simulation.submit.instance",
        "The created instance does not contain the exact compiled resource manifest",
        "Register member_role, room_channel, welcome_panel, and hub_panel exactly once",
    )
}

pub(super) fn identity_error() -> StructuredError {
    simulation_error(
        "INTENT_SIMULATION_IDENTITY_SETUP_FAILED",
        "intent.simulation.identity",
        "The deterministic simulation identity could not be created",
        "Stop candidate processing and report the harness configuration error",
    )
}

pub(super) fn simulation_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}
