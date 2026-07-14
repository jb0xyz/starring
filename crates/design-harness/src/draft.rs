use std::collections::{BTreeMap, BTreeSet};

use automation_core::validate::{validate_structural, ValidationError};
use automation_state::{
    ActionSpec, ButtonRoute, ChannelRef, InstanceRef, InteractionRuleSet, OverwriteTargetSpec,
    RoleRef, TriggerSpec,
};
use resource_resolution::ResourceBindingMap;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub ruleset: InteractionRuleSet,
    pub draft_revision: u64,
    pub validated_revision: Option<u64>,
    pub simulated_revision: Option<u64>,
}

impl Draft {
    pub fn new() -> Self {
        Self {
            ruleset: InteractionRuleSet {
                version: 1,
                panels: Vec::new(),
                modals: Vec::new(),
                rules: Vec::new(),
            },
            draft_revision: 0,
            validated_revision: None,
            simulated_revision: None,
        }
    }

    pub fn summary(&self) -> DraftSummary {
        self.summary_for_bindings(None)
    }

    pub(crate) fn summary_with_bindings(&self, bindings: &ResourceBindingMap) -> DraftSummary {
        self.summary_for_bindings(Some(bindings))
    }

    fn summary_for_bindings(&self, bindings: Option<&ResourceBindingMap>) -> DraftSummary {
        DraftSummary {
            panels: self.ruleset.panels.len(),
            modals: self.ruleset.modals.len(),
            rules: self.ruleset.rules.len(),
            actions: self
                .ruleset
                .rules
                .iter()
                .map(|rule| rule.actions.len())
                .sum(),
            unresolved_references: unresolved_references(&self.ruleset, bindings),
        }
    }

    pub(crate) fn mark_mutated(&mut self) {
        self.draft_revision += 1;
        self.validated_revision = None;
        self.simulated_revision = None;
    }

    pub(crate) fn validation_status(&self) -> String {
        if self.validated_revision == Some(self.draft_revision) {
            "current".to_string()
        } else {
            "stale".to_string()
        }
    }

    pub(crate) fn simulation_status(&self) -> String {
        if self.simulated_revision == Some(self.draft_revision) {
            "current".to_string()
        } else {
            "stale".to_string()
        }
    }

    pub(crate) fn newly_unresolved_after(&self, candidate: &Self) -> Vec<String> {
        let before: BTreeSet<String> = unresolved_references(&self.ruleset, None)
            .into_iter()
            .collect();
        unresolved_references(&candidate.ruleset, None)
            .into_iter()
            .filter(|reference| !before.contains(reference))
            .collect()
    }

    pub(crate) fn newly_dangling_after(&self, candidate: &Self) -> Vec<ValidationError> {
        let before = validate_structural(&self.ruleset).err().unwrap_or_default();
        validate_structural(&candidate.ruleset)
            .err()
            .unwrap_or_default()
            .into_iter()
            .filter(is_dangling_error)
            .filter(|error| !before.contains(error))
            .collect()
    }
}

impl Default for Draft {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftSummary {
    pub panels: usize,
    pub modals: usize,
    pub rules: usize,
    pub actions: usize,
    pub unresolved_references: Vec<String>,
}

fn unresolved_references(
    ruleset: &InteractionRuleSet,
    bindings: Option<&ResourceBindingMap>,
) -> Vec<String> {
    let mut unresolved = BTreeSet::new();
    for panel in &ruleset.panels {
        if !channel_is_bound(&panel.channel.0, bindings) {
            unresolved.insert(panel.channel.0.clone());
        }
    }
    let panel_buttons: BTreeSet<&str> = ruleset
        .panels
        .iter()
        .flat_map(|panel| panel.buttons.iter())
        .filter_map(|button| match &button.route {
            ButtonRoute::Static { key } => Some(key.as_str()),
            ButtonRoute::InstanceAction { .. } => None,
        })
        .collect();
    let modal_keys: BTreeSet<&str> = ruleset
        .modals
        .iter()
        .map(|modal| modal.key.as_str())
        .collect();
    let mut post_panel_buttons = BTreeSet::new();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            if let ActionSpec::PostPanel { buttons, .. } = action {
                for button in buttons {
                    if let ButtonRoute::Static { key } = &button.route {
                        post_panel_buttons.insert(key.as_str());
                    }
                }
            }
        }
    }

    for rule in &ruleset.rules {
        match &rule.trigger {
            TriggerSpec::ButtonClick { component }
                if !panel_buttons.contains(component.as_str())
                    && !post_panel_buttons.contains(component.as_str()) =>
            {
                unresolved.insert(component.clone());
            }
            TriggerSpec::ModalSubmit { modal } if !modal_keys.contains(modal.as_str()) => {
                unresolved.insert(modal.clone());
            }
            TriggerSpec::ButtonClick { .. }
            | TriggerSpec::ModalSubmit { .. }
            | TriggerSpec::InstanceAction { .. } => {}
        }

        let mut created: BTreeMap<&str, &str> = rule
            .actions
            .iter()
            .filter_map(|action| match action {
                ActionSpec::RegisterInstance { key, .. } => Some((key.as_str(), "instance")),
                _ => None,
            })
            .collect();
        for action in &rule.actions {
            match action {
                ActionSpec::CreateRole { key, .. } => {
                    created.insert(key.as_str(), "role");
                }
                ActionSpec::CreateChannel { key, .. } => {
                    created.insert(key.as_str(), "channel");
                }
                ActionSpec::PostPanel { key, .. } => {
                    created.insert(key.as_str(), "message");
                }
                ActionSpec::RegisterInstance { .. } => {}
                _ => {}
            }
            collect_action_references(action, &created, &modal_keys, bindings, &mut unresolved);
        }
    }
    unresolved.into_iter().collect()
}

fn is_dangling_error(error: &ValidationError) -> bool {
    matches!(
        error,
        ValidationError::UnknownButtonRef { .. }
            | ValidationError::UnknownModalRef { .. }
            | ValidationError::UnknownRoleRef { .. }
            | ValidationError::UnknownChannelRef { .. }
            | ValidationError::UnknownCreatedRoleRef { .. }
            | ValidationError::UnknownCreatedChannelRef { .. }
            | ValidationError::UnknownCreatedMessageRef { .. }
            | ValidationError::UnknownCreatedInstanceRef { .. }
            | ValidationError::CreatedRoleRefTypeMismatch { .. }
            | ValidationError::CreatedChannelRefTypeMismatch { .. }
            | ValidationError::CreatedMessageRefTypeMismatch { .. }
            | ValidationError::CreatedInstanceRefTypeMismatch { .. }
            | ValidationError::UnknownTemplateInput { .. }
            | ValidationError::InputTemplateInButtonRule { .. }
            | ValidationError::InstanceResourceMissingFromManifest { .. }
            | ValidationError::InstanceResourceProducedAfterRegister { .. }
    )
}

fn collect_action_references(
    action: &ActionSpec,
    created: &BTreeMap<&str, &str>,
    modal_keys: &BTreeSet<&str>,
    bindings: Option<&ResourceBindingMap>,
    unresolved: &mut BTreeSet<String>,
) {
    match action {
        ActionSpec::GrantRole { role, .. } => collect_role_ref(role, created, bindings, unresolved),
        ActionSpec::OpenModal { modal } if !modal_keys.contains(modal.as_str()) => {
            unresolved.insert(modal.clone());
        }
        ActionSpec::UpsertOverwrite {
            channel, target, ..
        } => {
            collect_channel_ref(channel, created, bindings, unresolved);
            if let OverwriteTargetSpec::Role(role) = target {
                collect_role_ref(role, created, bindings, unresolved);
            }
        }
        ActionSpec::PostPanel {
            channel, buttons, ..
        } => {
            collect_channel_ref(channel, created, bindings, unresolved);
            for button in buttons {
                if let ButtonRoute::InstanceAction { instance, .. } = &button.route {
                    collect_instance_ref(instance, created, unresolved);
                }
            }
        }
        ActionSpec::RegisterInstance { resources, .. } => {
            for reference in resources
                .roles
                .values()
                .chain(resources.channels.values())
                .chain(resources.messages.values())
            {
                if !created.contains_key(reference.created.as_str()) {
                    unresolved.insert(reference.created.clone());
                }
            }
        }
        ActionSpec::TeardownInstance { instance } => {
            collect_instance_ref(instance, created, unresolved);
        }
        _ => {}
    }
}

fn collect_role_ref(
    reference: &RoleRef,
    created: &BTreeMap<&str, &str>,
    bindings: Option<&ResourceBindingMap>,
    unresolved: &mut BTreeSet<String>,
) {
    match reference {
        RoleRef::Created(reference) if created.get(reference.created.as_str()) != Some(&"role") => {
            unresolved.insert(reference.created.clone());
        }
        RoleRef::Instance { instance, .. } => collect_instance_ref(instance, created, unresolved),
        RoleRef::Existing(key)
            if !bindings.is_some_and(|bindings| bindings.role_bindings.contains_key(key)) =>
        {
            unresolved.insert(key.0.clone());
        }
        RoleRef::Existing(_) | RoleRef::Created(_) => {}
    }
}

fn collect_channel_ref(
    reference: &ChannelRef,
    created: &BTreeMap<&str, &str>,
    bindings: Option<&ResourceBindingMap>,
    unresolved: &mut BTreeSet<String>,
) {
    match reference {
        ChannelRef::Created(reference) => {
            if created.get(reference.created.as_str()) != Some(&"channel") {
                unresolved.insert(reference.created.clone());
            }
        }
        ChannelRef::Existing(key) => {
            if !channel_is_bound(&key.0, bindings) {
                unresolved.insert(key.0.clone());
            }
        }
    }
}

fn channel_is_bound(key: &str, bindings: Option<&ResourceBindingMap>) -> bool {
    bindings.map_or(key == "study_hub", |bindings| {
        bindings
            .channel_bindings
            .keys()
            .any(|candidate| candidate.0 == key)
    })
}

fn collect_instance_ref(
    reference: &InstanceRef,
    created: &BTreeMap<&str, &str>,
    unresolved: &mut BTreeSet<String>,
) {
    if let InstanceRef::Created(reference) = reference {
        if created.get(reference.created.as_str()) != Some(&"instance") {
            unresolved.insert(reference.created.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn explicit_bindings_resolve_arbitrary_channels_and_roles() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version": 1,
            "panels": [{
                "key": "panel",
                "channel": "community_hub",
                "content": "Rooms",
                "buttons": []
            }],
            "modals": [],
            "rules": [{
                "key": "join",
                "trigger": {"type": "instance_action", "action": "join"},
                "actions": [{
                    "type": "grant_role",
                    "role": "existing_member",
                    "target": "actor"
                }]
            }]
        }))
        .unwrap();
        assert_eq!(
            draft.summary().unresolved_references,
            ["community_hub", "existing_member"]
        );

        let mut bindings = ResourceBindingMap::default();
        bindings.channel_bindings.insert(
            serde_json::from_value(json!("community_hub")).unwrap(),
            "700".parse().unwrap(),
        );
        bindings.role_bindings.insert(
            serde_json::from_value(json!("existing_member")).unwrap(),
            "701".parse().unwrap(),
        );

        assert!(draft
            .summary_with_bindings(&bindings)
            .unresolved_references
            .is_empty());
    }

    #[test]
    fn legacy_summary_keeps_the_study_hub_binding() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version": 1,
            "panels": [{
                "key": "panel",
                "channel": "study_hub",
                "content": "Rooms",
                "buttons": []
            }],
            "modals": [],
            "rules": []
        }))
        .unwrap();

        assert!(draft.summary().unresolved_references.is_empty());
    }
}
