use crate::turn::{
    ScopeAction, ScopeActionTarget, ScopeButtonRoute, ScopeInstanceRef, ScopeInstanceResources,
    ScopeManifestEntry, ScopeModalField, ScopeModalFieldStyle, ScopeOverwriteTarget,
    ScopePermission, ScopePostPanelButton, ScopePostPanelButtonRoute, ScopeRequirement,
    ScopeResourceRef, ScopeRoleRef, ScopeTrigger,
};

use super::catalog::private_study_room_descriptor;
use super::compile::RecipeExpansion;
use super::keyspace::IntentKeyspace;
use super::model::{ResolvedCloseControlV1, ResolvedManagedPrivateRoomV1, RoomNamePatternV1};
use super::provenance::ProvenanceBuilder;

const ROOM_NAME_INPUT: &str = "room_name";
const INSTANCE_KIND: &str = "study_room";
const MEMBER_ROLE_ALIAS: &str = "member_role";
const ROOM_CHANNEL_ALIAS: &str = "room_channel";
const WELCOME_PANEL_ALIAS: &str = "welcome_panel";
const HUB_PANEL_ALIAS: &str = "hub_panel";

const BASE_GENERATED_SYMBOLS: &[&str] = &[
    "study_panel",
    "create_study_room",
    "study_modal",
    "open_modal",
    "submit_room",
    "member_role",
    "room_channel",
    "welcome_panel",
    "hub_panel",
    "study_instance",
    "study_help",
    "show_help",
    "join",
    "join_room",
];

pub(super) fn compile_private_study_room(
    feature_id: &str,
    room: &ResolvedManagedPrivateRoomV1,
) -> RecipeExpansion {
    let descriptor = private_study_room_descriptor();
    let keyspace = IntentKeyspace::new(feature_id);
    let mut requirements = Vec::with_capacity(descriptor.max_requirements);
    let mut provenance = ProvenanceBuilder::new(feature_id, descriptor.id, descriptor.version);

    let panel = keyspace.symbol("study_panel");
    let create_button = keyspace.symbol("create_study_room");
    let modal = keyspace.symbol("study_modal");
    let open_rule = keyspace.symbol("open_modal");
    let submit_rule = keyspace.symbol("submit_room");
    let member_role = keyspace.symbol("member_role");
    let room_channel = keyspace.symbol("room_channel");
    let welcome_panel = keyspace.symbol("welcome_panel");
    let hub_panel = keyspace.symbol("hub_panel");
    let instance = keyspace.symbol("study_instance");
    let help_button = keyspace.symbol("study_help");
    let help_rule = keyspace.symbol("show_help");
    let join_action = keyspace.symbol("join");
    let join_rule = keyspace.symbol("join_room");

    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Panel {
            id: id(feature_id, "surface.panel"),
            key: panel.clone(),
            channel: room.hub_channel.value.as_str().to_string(),
            content: room.copy.launcher_content.value.clone(),
        },
        "launcher_surface",
        &["hub_channel", "copy.launcher_content"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Button {
            id: id(feature_id, "surface.create_button"),
            panel_key: panel,
            label: room.copy.create_button_label.value.clone(),
            route: ScopeButtonRoute::Static {
                key: create_button.clone(),
            },
        },
        "launcher_surface",
        &["copy.create_button_label"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Modal {
            id: id(feature_id, "surface.modal"),
            key: modal.clone(),
            title: room.copy.modal_title.value.clone(),
            fields: vec![ScopeModalField {
                key: ROOM_NAME_INPUT.to_string(),
                label: room.copy.room_name_label.value.clone(),
                style: ScopeModalFieldStyle::Short,
                required: true,
            }],
        },
        "room_name_input",
        &["copy.modal_title", "copy.room_name_label"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Rule {
            id: id(feature_id, "open.rule"),
            key: open_rule.clone(),
            trigger: ScopeTrigger::ButtonClick {
                component: create_button,
            },
        },
        "open_room_modal",
        &["recipe.open_room_modal"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "open.open_modal"),
        &open_rule,
        ScopeAction::OpenModal {
            modal: modal.clone(),
        },
        "open_room_modal",
        &["recipe.open_room_modal"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Rule {
            id: id(feature_id, "submit.rule"),
            key: submit_rule.clone(),
            trigger: ScopeTrigger::ModalSubmit { modal },
        },
        "create_private_room",
        &["recipe.create_private_room"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.defer"),
        &submit_rule,
        ScopeAction::DeferEphemeral,
        "create_private_room",
        &["recipe.deferred_response"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.create_member_role"),
        &submit_rule,
        ScopeAction::CreateRole {
            key: member_role.clone(),
            name: render_pattern(&room.naming.member_role_name.value),
        },
        "private_membership",
        &["naming.member_role_name"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.create_room_channel"),
        &submit_rule,
        ScopeAction::CreateChannel {
            key: room_channel.clone(),
            name: render_pattern(&room.naming.channel_name.value),
        },
        "private_membership",
        &["naming.channel_name"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.deny_everyone_view"),
        &submit_rule,
        ScopeAction::UpsertOverwrite {
            channel: ScopeResourceRef::Created {
                name: room_channel.clone(),
            },
            target: ScopeOverwriteTarget::Everyone,
            allow: vec![],
            deny: vec![ScopePermission::ViewChannel],
        },
        "private_membership",
        &["recipe.private_visibility"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.allow_member_view"),
        &submit_rule,
        ScopeAction::UpsertOverwrite {
            channel: ScopeResourceRef::Created {
                name: room_channel.clone(),
            },
            target: ScopeOverwriteTarget::Role {
                role: ScopeRoleRef::Created {
                    name: member_role.clone(),
                },
            },
            allow: vec![ScopePermission::ViewChannel],
            deny: vec![],
        },
        "private_membership",
        &["recipe.private_visibility"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.grant_creator"),
        &submit_rule,
        ScopeAction::GrantRole {
            role: ScopeRoleRef::Created {
                name: member_role.clone(),
            },
            target: ScopeActionTarget::Actor,
        },
        "private_membership",
        &["recipe.creator_membership"],
    );

    let mut welcome_buttons = vec![ScopePostPanelButton {
        label: room.controls.help.label.value.clone(),
        route: ScopePostPanelButtonRoute::Static {
            key: help_button.clone(),
        },
    }];
    let close = match &room.controls.close {
        ResolvedCloseControlV1::Disabled { .. } => None,
        ResolvedCloseControlV1::AnyMember {
            label, response, ..
        } => {
            let action = keyspace.symbol("close");
            welcome_buttons.push(ScopePostPanelButton {
                label: label.value.clone(),
                route: ScopePostPanelButtonRoute::InstanceAction {
                    instance: ScopeInstanceRef::Created {
                        name: instance.clone(),
                    },
                    action: action.clone(),
                },
            });
            Some((action, response.value.clone()))
        }
    };
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.post_welcome"),
        &submit_rule,
        ScopeAction::PostPanel {
            key: welcome_panel.clone(),
            channel: ScopeResourceRef::Created {
                name: room_channel.clone(),
            },
            content: render_pattern(&room.copy.welcome_content.value),
            buttons: welcome_buttons,
        },
        "room_controls",
        &["copy.welcome_content", "controls.help", "controls.close"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.post_hub"),
        &submit_rule,
        ScopeAction::PostPanel {
            key: hub_panel.clone(),
            channel: ScopeResourceRef::Existing {
                name: room.hub_channel.value.as_str().to_string(),
            },
            content: render_pattern(&room.copy.hub_announcement.value),
            buttons: vec![ScopePostPanelButton {
                label: room.controls.join.label.value.clone(),
                route: ScopePostPanelButtonRoute::InstanceAction {
                    instance: ScopeInstanceRef::Created {
                        name: instance.clone(),
                    },
                    action: join_action.clone(),
                },
            }],
        },
        "room_discovery",
        &["hub_channel", "copy.hub_announcement", "controls.join"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.register_instance"),
        &submit_rule,
        ScopeAction::RegisterInstance {
            key: instance,
            instance_kind: INSTANCE_KIND.to_string(),
            resources: ScopeInstanceResources {
                roles: vec![ScopeManifestEntry {
                    alias: MEMBER_ROLE_ALIAS.to_string(),
                    created: member_role,
                }],
                channels: vec![ScopeManifestEntry {
                    alias: ROOM_CHANNEL_ALIAS.to_string(),
                    created: room_channel,
                }],
                messages: vec![
                    ScopeManifestEntry {
                        alias: WELCOME_PANEL_ALIAS.to_string(),
                        created: welcome_panel,
                    },
                    ScopeManifestEntry {
                        alias: HUB_PANEL_ALIAS.to_string(),
                        created: hub_panel,
                    },
                ],
            },
        },
        "instance_lifecycle",
        &["recipe.instance_manifest"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "submit.complete"),
        &submit_rule,
        ScopeAction::EditResponse {
            content: render_pattern(&room.copy.completed_response.value),
        },
        "creation_feedback",
        &["copy.completed_response"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Rule {
            id: id(feature_id, "help.rule"),
            key: help_rule.clone(),
            trigger: ScopeTrigger::ButtonClick {
                component: help_button,
            },
        },
        "help_control",
        &["controls.help"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "help.respond"),
        &help_rule,
        ScopeAction::RespondEphemeral {
            content: room.controls.help.response.value.clone(),
        },
        "help_control",
        &["controls.help.response"],
    );
    push(
        &mut requirements,
        &mut provenance,
        ScopeRequirement::Rule {
            id: id(feature_id, "join.rule"),
            key: join_rule.clone(),
            trigger: ScopeTrigger::InstanceAction {
                action: join_action,
            },
        },
        "join_control",
        &["controls.join"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "join.defer"),
        &join_rule,
        ScopeAction::DeferEphemeral,
        "join_control",
        &["recipe.deferred_response"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "join.grant_member"),
        &join_rule,
        ScopeAction::GrantRole {
            role: ScopeRoleRef::Instance {
                instance: ScopeInstanceRef::Event,
                alias: MEMBER_ROLE_ALIAS.to_string(),
            },
            target: ScopeActionTarget::Actor,
        },
        "join_control",
        &["recipe.join_membership"],
    );
    push_action(
        &mut requirements,
        &mut provenance,
        id(feature_id, "join.respond"),
        &join_rule,
        ScopeAction::EditResponse {
            content: room.controls.join.response.value.clone(),
        },
        "join_control",
        &["controls.join.response"],
    );

    let close_enabled = close.is_some();
    if let Some((close_action, close_response)) = close {
        let close_rule = keyspace.symbol("close_room");
        push(
            &mut requirements,
            &mut provenance,
            ScopeRequirement::Rule {
                id: id(feature_id, "close.rule"),
                key: close_rule.clone(),
                trigger: ScopeTrigger::InstanceAction {
                    action: close_action,
                },
            },
            "close_control",
            &["controls.close.policy"],
        );
        push_action(
            &mut requirements,
            &mut provenance,
            id(feature_id, "close.defer"),
            &close_rule,
            ScopeAction::DeferEphemeral,
            "close_control",
            &["recipe.deferred_response"],
        );
        push_action(
            &mut requirements,
            &mut provenance,
            id(feature_id, "close.teardown"),
            &close_rule,
            ScopeAction::TeardownInstance {
                instance: ScopeInstanceRef::Event,
            },
            "close_control",
            &["controls.close.policy"],
        );
        push_action(
            &mut requirements,
            &mut provenance,
            id(feature_id, "close.respond"),
            &close_rule,
            ScopeAction::EditResponse {
                content: close_response,
            },
            "close_control",
            &["controls.close.response"],
        );
    }

    let mut generated_symbols = BASE_GENERATED_SYMBOLS.to_vec();
    if close_enabled {
        generated_symbols.extend(["close", "close_room"]);
    }
    RecipeExpansion {
        descriptor,
        feature_id: feature_id.to_string(),
        requirements,
        provenance,
        generated_objects: keyspace.generated_objects(&generated_symbols),
        external_channel_bindings: vec![room.hub_channel.value.as_str().to_string()],
    }
}

fn push(
    requirements: &mut Vec<ScopeRequirement>,
    provenance: &mut ProvenanceBuilder,
    requirement: ScopeRequirement,
    clause: &str,
    intent_paths: &[&str],
) {
    provenance.record(requirement.id(), clause, intent_paths);
    requirements.push(requirement);
}

fn push_action(
    requirements: &mut Vec<ScopeRequirement>,
    provenance: &mut ProvenanceBuilder,
    requirement_id: String,
    rule_key: &str,
    action: ScopeAction,
    clause: &str,
    intent_paths: &[&str],
) {
    push(
        requirements,
        provenance,
        ScopeRequirement::Action {
            id: requirement_id,
            rule_key: rule_key.to_string(),
            action,
            minimum: 1,
        },
        clause,
        intent_paths,
    );
}

fn id(feature_id: &str, local_id: &str) -> String {
    format!("{feature_id}.{local_id}")
}

fn render_pattern(pattern: &RoomNamePatternV1) -> String {
    format!(
        "{}${{input.{ROOM_NAME_INPUT}}}{}",
        pattern.prefix, pattern.suffix
    )
}
