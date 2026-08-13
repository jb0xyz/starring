use std::fmt::{Display, Formatter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::canonical::{canonical_json_bytes, framed_sha256};
use crate::model::{AUTOMATION_SPEC_KIND_V1, AUTOMATION_SPEC_SCHEMA_VERSION_V1};
use crate::simulate::{MAX_SIMULATION_INPUT_BYTES_V1, MAX_SIMULATION_PAYLOAD_BYTES_V1};
use crate::validate::{
    MAX_ACTIONS_PER_WORKFLOW_V1, MAX_AUTOMATION_DESCRIPTION_BYTES_V1,
    MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1, MAX_AUTOMATION_PANELS_V1,
    MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1, MAX_AUTOMATION_WORKFLOWS_V1, MAX_BUTTON_LABEL_CHARS_V1,
    MAX_CONDITION_DEPTH_V1, MAX_CONDITION_NODES_V1, MAX_DISCORD_CUSTOM_ID_BYTES_V1,
    MAX_IDENTIFIER_BYTES_V1, MAX_INSTANCE_ACTION_ID_BYTES_V1, MAX_INSTANCE_RESOURCE_ALIASES_V1,
    MAX_MODAL_DEFINITIONS_V1, MAX_MODAL_FIELDS_V1, MAX_MODAL_FIELD_LABEL_CHARS_V1,
    MAX_MODAL_INPUT_UTF16_UNITS_V1, MAX_MODAL_TITLE_CHARS_V1, MAX_PANEL_BUTTONS_V1,
    MAX_PANEL_CONTENT_CHARS_V1, MAX_RESOURCE_ALIAS_BYTES_V1, MAX_RESOURCE_NAME_TEMPLATE_CHARS_V1,
    MAX_SIMULATION_INPUTS_V1, MAX_SIMULATION_INPUT_UTF16_UNITS_V1, MAX_TEMPLATE_SOURCE_CHARS_V1,
    MAX_TOTAL_ACTIONS_V1,
};

pub const AUTOMATION_SPEC_DESCRIPTOR_KIND_V1: &str = "starring.automation-spec-descriptor.v1";
pub const AUTOMATION_SPEC_DESCRIPTOR_REVISION_V1: u32 = 1;
pub const MAX_AUTOMATION_SPEC_PREVIEW_REQUEST_BYTES_V1: usize = 48 * 1_024;
pub const MAX_AUTOMATION_SPEC_SIMULATION_REQUEST_BYTES_V1: usize = 256 * 1_024;
const DESCRIPTOR_DOMAIN_V1: &[u8] = b"starring.automation_spec_descriptor.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AutomationSpecDescriptorDigestV1([u8; 32]);

impl AutomationSpecDescriptorDigestV1 {
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

impl Display for AutomationSpecDescriptorDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl Serialize for AutomationSpecDescriptorDigestV1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for AutomationSpecDescriptorDigestV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom("expected a 64-character lowercase descriptor digest")
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPrimitiveRuntimeSupportV1 {
    InteractionRuntimeV1,
    PreviewAndSimulationOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationEffectClassV1 {
    None,
    InteractionResponse,
    CompensatableExternalWrite,
    NonCompensatableExternalWrite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationPrimitiveDescriptorV1 {
    pub name: String,
    pub runtime_support: AutomationPrimitiveRuntimeSupportV1,
    pub effect_class: AutomationEffectClassV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecLimitsV1 {
    pub maximum_encoded_spec_bytes: u32,
    pub maximum_preview_request_bytes: u32,
    pub maximum_simulation_request_bytes: u32,
    pub maximum_identifier_bytes: u16,
    pub maximum_instance_action_id_bytes: u16,
    pub maximum_resource_alias_bytes: u16,
    pub maximum_discord_custom_id_bytes: u16,
    pub maximum_display_name_characters: u16,
    pub maximum_description_bytes: u16,
    pub maximum_panels: u16,
    pub maximum_panel_content_characters: u16,
    pub maximum_buttons_per_panel: u16,
    pub maximum_button_label_characters: u16,
    pub maximum_modals: u16,
    pub maximum_fields_per_modal: u16,
    pub maximum_modal_title_characters: u16,
    pub maximum_modal_field_label_characters: u16,
    pub maximum_modal_input_utf16_units: u16,
    pub maximum_workflows: u16,
    pub maximum_actions_per_workflow: u16,
    pub maximum_total_actions: u16,
    pub maximum_condition_depth: u16,
    pub maximum_condition_nodes: u16,
    pub maximum_template_source_characters: u16,
    pub maximum_resource_name_template_characters: u16,
    pub maximum_instance_resource_aliases: u16,
    pub maximum_simulation_inputs: u16,
    pub maximum_simulation_input_bytes: u16,
    pub maximum_simulation_input_utf16_units: u16,
    pub maximum_simulation_payload_bytes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecSafetyV1 {
    pub arbitrary_code: bool,
    pub arbitrary_http: bool,
    pub event_time_llm: bool,
    pub secret_reference_fields: bool,
    pub loops: bool,
    pub recursion: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecDescriptorV1 {
    pub schema_version: u16,
    pub kind: String,
    pub descriptor_revision: u32,
    pub descriptor_digest: AutomationSpecDescriptorDigestV1,
    pub automation_spec_kind: String,
    pub triggers: Vec<AutomationPrimitiveDescriptorV1>,
    pub conditions: Vec<AutomationPrimitiveDescriptorV1>,
    pub actions: Vec<AutomationPrimitiveDescriptorV1>,
    pub capabilities: Vec<String>,
    pub limits: AutomationSpecLimitsV1,
    pub safety: AutomationSpecSafetyV1,
    pub installation_readiness: String,
    pub simulation_input_stage: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct AutomationSpecDescriptorMaterialV1 {
    schema_version: u16,
    kind: String,
    descriptor_revision: u32,
    automation_spec_kind: String,
    triggers: Vec<AutomationPrimitiveDescriptorV1>,
    conditions: Vec<AutomationPrimitiveDescriptorV1>,
    actions: Vec<AutomationPrimitiveDescriptorV1>,
    capabilities: Vec<String>,
    limits: AutomationSpecLimitsV1,
    safety: AutomationSpecSafetyV1,
    installation_readiness: String,
    simulation_input_stage: String,
}

pub fn automation_spec_descriptor_v1() -> AutomationSpecDescriptorV1 {
    let material = descriptor_material_v1();
    let bytes = canonical_json_bytes(&material).expect("static descriptor must serialize");
    AutomationSpecDescriptorV1 {
        schema_version: material.schema_version,
        kind: material.kind,
        descriptor_revision: material.descriptor_revision,
        descriptor_digest: AutomationSpecDescriptorDigestV1(framed_sha256(
            DESCRIPTOR_DOMAIN_V1,
            &bytes,
        )),
        automation_spec_kind: material.automation_spec_kind,
        triggers: material.triggers,
        conditions: material.conditions,
        actions: material.actions,
        capabilities: material.capabilities,
        limits: material.limits,
        safety: material.safety,
        installation_readiness: material.installation_readiness,
        simulation_input_stage: material.simulation_input_stage,
    }
}

fn descriptor_material_v1() -> AutomationSpecDescriptorMaterialV1 {
    AutomationSpecDescriptorMaterialV1 {
        schema_version: AUTOMATION_SPEC_SCHEMA_VERSION_V1,
        kind: AUTOMATION_SPEC_DESCRIPTOR_KIND_V1.to_string(),
        descriptor_revision: AUTOMATION_SPEC_DESCRIPTOR_REVISION_V1,
        automation_spec_kind: AUTOMATION_SPEC_KIND_V1.to_string(),
        triggers: ["button_click", "modal_submit", "instance_action"]
            .into_iter()
            .map(|name| primitive(name, true, AutomationEffectClassV1::None))
            .collect(),
        conditions: [
            "always",
            "input_non_empty",
            "input_equals",
            "all",
            "any",
            "not",
        ]
        .into_iter()
        .map(|name| primitive(name, name == "always", AutomationEffectClassV1::None))
        .collect(),
        actions: vec![
            primitive(
                "grant_role",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "respond_ephemeral",
                true,
                AutomationEffectClassV1::InteractionResponse,
            ),
            primitive(
                "open_modal",
                true,
                AutomationEffectClassV1::InteractionResponse,
            ),
            primitive(
                "create_channel",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "create_role",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "upsert_overwrite",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "post_panel",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "defer_ephemeral",
                true,
                AutomationEffectClassV1::InteractionResponse,
            ),
            primitive(
                "edit_response",
                true,
                AutomationEffectClassV1::NonCompensatableExternalWrite,
            ),
            primitive(
                "register_instance",
                true,
                AutomationEffectClassV1::CompensatableExternalWrite,
            ),
            primitive(
                "teardown_instance",
                true,
                AutomationEffectClassV1::NonCompensatableExternalWrite,
            ),
        ],
        capabilities: [
            "interaction_response",
            "manage_channels",
            "manage_instances",
            "manage_roles",
            "post_messages",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        limits: AutomationSpecLimitsV1 {
            maximum_encoded_spec_bytes: MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1 as u32,
            maximum_preview_request_bytes: MAX_AUTOMATION_SPEC_PREVIEW_REQUEST_BYTES_V1 as u32,
            maximum_simulation_request_bytes: MAX_AUTOMATION_SPEC_SIMULATION_REQUEST_BYTES_V1
                as u32,
            maximum_identifier_bytes: MAX_IDENTIFIER_BYTES_V1 as u16,
            maximum_instance_action_id_bytes: MAX_INSTANCE_ACTION_ID_BYTES_V1 as u16,
            maximum_resource_alias_bytes: MAX_RESOURCE_ALIAS_BYTES_V1 as u16,
            maximum_discord_custom_id_bytes: MAX_DISCORD_CUSTOM_ID_BYTES_V1 as u16,
            maximum_display_name_characters: MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1 as u16,
            maximum_description_bytes: MAX_AUTOMATION_DESCRIPTION_BYTES_V1 as u16,
            maximum_panels: MAX_AUTOMATION_PANELS_V1 as u16,
            maximum_panel_content_characters: MAX_PANEL_CONTENT_CHARS_V1 as u16,
            maximum_buttons_per_panel: MAX_PANEL_BUTTONS_V1 as u16,
            maximum_button_label_characters: MAX_BUTTON_LABEL_CHARS_V1 as u16,
            maximum_modals: MAX_MODAL_DEFINITIONS_V1 as u16,
            maximum_fields_per_modal: MAX_MODAL_FIELDS_V1 as u16,
            maximum_modal_title_characters: MAX_MODAL_TITLE_CHARS_V1 as u16,
            maximum_modal_field_label_characters: MAX_MODAL_FIELD_LABEL_CHARS_V1 as u16,
            maximum_modal_input_utf16_units: MAX_MODAL_INPUT_UTF16_UNITS_V1 as u16,
            maximum_workflows: MAX_AUTOMATION_WORKFLOWS_V1 as u16,
            maximum_actions_per_workflow: MAX_ACTIONS_PER_WORKFLOW_V1 as u16,
            maximum_total_actions: MAX_TOTAL_ACTIONS_V1 as u16,
            maximum_condition_depth: MAX_CONDITION_DEPTH_V1 as u16,
            maximum_condition_nodes: MAX_CONDITION_NODES_V1 as u16,
            maximum_template_source_characters: MAX_TEMPLATE_SOURCE_CHARS_V1 as u16,
            maximum_resource_name_template_characters: MAX_RESOURCE_NAME_TEMPLATE_CHARS_V1 as u16,
            maximum_instance_resource_aliases: MAX_INSTANCE_RESOURCE_ALIASES_V1 as u16,
            maximum_simulation_inputs: MAX_SIMULATION_INPUTS_V1 as u16,
            maximum_simulation_input_bytes: MAX_SIMULATION_INPUT_BYTES_V1 as u16,
            maximum_simulation_input_utf16_units: MAX_SIMULATION_INPUT_UTF16_UNITS_V1 as u16,
            maximum_simulation_payload_bytes: MAX_SIMULATION_PAYLOAD_BYTES_V1 as u32,
        },
        safety: AutomationSpecSafetyV1 {
            arbitrary_code: false,
            arbitrary_http: false,
            event_time_llm: false,
            secret_reference_fields: false,
            loops: false,
            recursion: false,
        },
        installation_readiness: "not_evaluated".to_string(),
        simulation_input_stage: "post_gateway_admission".to_string(),
    }
}

fn primitive(
    name: &str,
    runtime_supported: bool,
    effect_class: AutomationEffectClassV1,
) -> AutomationPrimitiveDescriptorV1 {
    AutomationPrimitiveDescriptorV1 {
        name: name.to_string(),
        runtime_support: if runtime_supported {
            AutomationPrimitiveRuntimeSupportV1::InteractionRuntimeV1
        } else {
            AutomationPrimitiveRuntimeSupportV1::PreviewAndSimulationOnly
        },
        effect_class,
    }
}
