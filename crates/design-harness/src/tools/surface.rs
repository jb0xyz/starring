use automation_state::{
    ButtonRoute, ButtonSpec, InstanceRef, InteractionRule, ModalFieldSpec, ModalFieldStyle,
    ModalSpec, PanelSpec, TriggerSpec,
};
use serde_json::Value;

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::{
    empty_update, find_rule_mut, missing_modal, missing_panel, missing_rule,
    reference_conversion_error, AddButtonInput, AddModalInput, AddPanelInput, BeginRuleInput,
    ButtonRouteInput, ModalFieldInput, ModalFieldStyleInput, RemoveButtonInput, RemoveModalInput,
    RemovePanelInput, RemoveRuleInput, TriggerInput, TriggerKindInput, UpdateButtonInput,
    UpdateModalInput, UpdatePanelInput, UpdateRuleInput,
};

pub(super) fn add_panel(
    draft: &mut Draft,
    input: AddPanelInput,
) -> Result<String, StructuredError> {
    let channel =
        serde_json::from_value(Value::String(input.channel)).map_err(reference_conversion_error)?;
    let key = input.key;
    draft.ruleset.panels.push(PanelSpec {
        key: key.clone(),
        channel,
        content: input.content,
        buttons: Vec::new(),
    });
    Ok(format!("Added panel {key}"))
}

pub(super) fn update_panel(
    draft: &mut Draft,
    input: UpdatePanelInput,
) -> Result<String, StructuredError> {
    if input.channel.is_none() && input.content.is_none() {
        return Err(empty_update("update_panel", "channel or content"));
    }
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.key)
        .ok_or_else(|| missing_panel(&input.key))?;
    if let Some(channel) = input.channel {
        panel.channel =
            serde_json::from_value(Value::String(channel)).map_err(reference_conversion_error)?;
    }
    if let Some(content) = input.content {
        panel.content = content;
    }
    Ok(format!("Updated panel {}", input.key))
}

pub(super) fn remove_panel(
    draft: &mut Draft,
    input: RemovePanelInput,
) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .panels
        .iter()
        .position(|panel| panel.key == input.key)
        .ok_or_else(|| missing_panel(&input.key))?;
    draft.ruleset.panels.remove(index);
    Ok(format!("Removed panel {}", input.key))
}

pub(super) fn add_button(
    draft: &mut Draft,
    input: AddButtonInput,
) -> Result<String, StructuredError> {
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    panel.buttons.push(ButtonSpec {
        label: input.label,
        route: declared_button_route(input.route),
    });
    Ok(format!("Added button to panel {}", input.panel_key))
}

pub(super) fn update_button(
    draft: &mut Draft,
    input: UpdateButtonInput,
) -> Result<String, StructuredError> {
    if input.label.is_none() && input.route.is_none() {
        return Err(empty_update("update_button", "label or route"));
    }
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    let index = button_index(panel, &input.selector, &input.panel_key)?;
    let button = &mut panel.buttons[index];
    if let Some(label) = input.label {
        button.label = label;
    }
    if let Some(route) = input.route {
        button.route = declared_button_route(route);
    }
    Ok(format!("Updated button in panel {}", input.panel_key))
}

pub(super) fn remove_button(
    draft: &mut Draft,
    input: RemoveButtonInput,
) -> Result<String, StructuredError> {
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    let index = button_index(panel, &input.selector, &input.panel_key)?;
    panel.buttons.remove(index);
    Ok(format!("Removed button from panel {}", input.panel_key))
}

pub(super) fn add_modal(
    draft: &mut Draft,
    input: AddModalInput,
) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.modals.push(ModalSpec {
        key: key.clone(),
        title: input.title,
        fields: input.fields.into_iter().map(modal_field).collect(),
    });
    Ok(format!("Added modal {key}"))
}

pub(super) fn update_modal(
    draft: &mut Draft,
    input: UpdateModalInput,
) -> Result<String, StructuredError> {
    if input.title.is_none() && input.fields.is_none() {
        return Err(empty_update("update_modal", "title or fields"));
    }
    let modal = draft
        .ruleset
        .modals
        .iter_mut()
        .find(|modal| modal.key == input.key)
        .ok_or_else(|| missing_modal(&input.key))?;
    if let Some(title) = input.title {
        modal.title = title;
    }
    if let Some(fields) = input.fields {
        modal.fields = fields.into_iter().map(modal_field).collect();
    }
    Ok(format!("Updated modal {}", input.key))
}

pub(super) fn remove_modal(
    draft: &mut Draft,
    input: RemoveModalInput,
) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .modals
        .iter()
        .position(|modal| modal.key == input.key)
        .ok_or_else(|| missing_modal(&input.key))?;
    draft.ruleset.modals.remove(index);
    Ok(format!("Removed modal {}", input.key))
}

pub(super) fn begin_rule(
    draft: &mut Draft,
    input: BeginRuleInput,
) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.rules.push(InteractionRule {
        key: key.clone(),
        trigger: begin_trigger(input.trigger_kind, input.trigger_ref),
        actions: Vec::new(),
    });
    Ok(format!("Began rule {key}"))
}

pub(super) fn update_rule(
    draft: &mut Draft,
    input: UpdateRuleInput,
) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.key)?;
    rule.trigger = trigger(input.trigger);
    Ok(format!("Updated rule {}", input.key))
}

pub(super) fn remove_rule(
    draft: &mut Draft,
    input: RemoveRuleInput,
) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .rules
        .iter()
        .position(|rule| rule.key == input.key)
        .ok_or_else(|| missing_rule(&input.key))?;
    draft.ruleset.rules.remove(index);
    Ok(format!("Removed rule {}", input.key))
}

fn modal_field(input: ModalFieldInput) -> ModalFieldSpec {
    ModalFieldSpec {
        key: input.key,
        label: input.label,
        style: match input.style {
            ModalFieldStyleInput::Short => ModalFieldStyle::Short,
            ModalFieldStyleInput::Paragraph => ModalFieldStyle::Paragraph,
        },
        required: input.required,
    }
}

fn trigger(input: TriggerInput) -> TriggerSpec {
    match input {
        TriggerInput::ButtonClick { component } => TriggerSpec::ButtonClick { component },
        TriggerInput::ModalSubmit { modal } => TriggerSpec::ModalSubmit { modal },
        TriggerInput::InstanceAction { action } => TriggerSpec::InstanceAction { action },
    }
}

fn begin_trigger(kind: TriggerKindInput, trigger_ref: String) -> TriggerSpec {
    match kind {
        TriggerKindInput::ButtonClick => TriggerSpec::ButtonClick {
            component: trigger_ref,
        },
        TriggerKindInput::ModalSubmit => TriggerSpec::ModalSubmit { modal: trigger_ref },
        TriggerKindInput::InstanceAction => TriggerSpec::InstanceAction {
            action: trigger_ref,
        },
    }
}

fn declared_button_route(input: ButtonRouteInput) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { key } => ButtonRoute::Static { key },
        ButtonRouteInput::InstanceAction { action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Event,
            action,
        },
    }
}

fn button_index(
    panel: &PanelSpec,
    selector: &ButtonRouteInput,
    panel_key: &str,
) -> Result<usize, StructuredError> {
    let matches: Vec<usize> = panel
        .buttons
        .iter()
        .enumerate()
        .filter_map(|(index, button)| {
            button_route_matches(&button.route, selector).then_some(index)
        })
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(StructuredError::new(
            "BUTTON_NOT_FOUND",
            format!("panel.{panel_key}.buttons"),
            "No button has the selected route",
            "Use the button's current static key or instance action as selector",
        )),
        _ => Err(StructuredError::new(
            "AMBIGUOUS_BUTTON_SELECTOR",
            format!("panel.{panel_key}.buttons"),
            "More than one button has the selected route",
            "Make button routes unique before editing by selector",
        )),
    }
}

fn button_route_matches(route: &ButtonRoute, selector: &ButtonRouteInput) -> bool {
    match (route, selector) {
        (ButtonRoute::Static { key }, ButtonRouteInput::Static { key: selected }) => {
            key == selected
        }
        (
            ButtonRoute::InstanceAction { action, .. },
            ButtonRouteInput::InstanceAction { action: selected },
        ) => action == selected,
        _ => false,
    }
}
