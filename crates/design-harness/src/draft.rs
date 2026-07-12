use std::collections::{BTreeMap, BTreeSet};

use automation_state::{
    ActionSpec, ButtonRoute, ChannelRef, InstanceRef, InteractionRuleSet, OverwriteTargetSpec,
    RoleRef, TriggerSpec,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq)]
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
            unresolved_references: unresolved_references(&self.ruleset),
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
}

impl Default for Draft {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftSummary {
    pub panels: usize,
    pub modals: usize,
    pub rules: usize,
    pub actions: usize,
    pub unresolved_references: Vec<String>,
}

fn unresolved_references(ruleset: &InteractionRuleSet) -> Vec<String> {
    let mut unresolved = BTreeSet::new();
    for panel in &ruleset.panels {
        if panel.channel.0 != "study_hub" {
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
            collect_action_references(action, &created, &modal_keys, &mut unresolved);
        }
    }
    unresolved.into_iter().collect()
}

fn collect_action_references(
    action: &ActionSpec,
    created: &BTreeMap<&str, &str>,
    modal_keys: &BTreeSet<&str>,
    unresolved: &mut BTreeSet<String>,
) {
    match action {
        ActionSpec::GrantRole { role, .. } => collect_role_ref(role, created, unresolved),
        ActionSpec::OpenModal { modal } if !modal_keys.contains(modal.as_str()) => {
            unresolved.insert(modal.clone());
        }
        ActionSpec::UpsertOverwrite {
            channel, target, ..
        } => {
            collect_channel_ref(channel, created, unresolved);
            if let OverwriteTargetSpec::Role(role) = target {
                collect_role_ref(role, created, unresolved);
            }
        }
        ActionSpec::PostPanel {
            channel, buttons, ..
        } => {
            collect_channel_ref(channel, created, unresolved);
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
    unresolved: &mut BTreeSet<String>,
) {
    match reference {
        RoleRef::Created(reference) if created.get(reference.created.as_str()) != Some(&"role") => {
            unresolved.insert(reference.created.clone());
        }
        RoleRef::Instance { instance, .. } => collect_instance_ref(instance, created, unresolved),
        RoleRef::Existing(key) => {
            unresolved.insert(key.0.clone());
        }
        RoleRef::Created(_) => {}
    }
}

fn collect_channel_ref(
    reference: &ChannelRef,
    created: &BTreeMap<&str, &str>,
    unresolved: &mut BTreeSet<String>,
) {
    match reference {
        ChannelRef::Created(reference) => {
            if created.get(reference.created.as_str()) != Some(&"channel") {
                unresolved.insert(reference.created.clone());
            }
        }
        ChannelRef::Existing(key) => {
            if key.0 != "study_hub" {
                unresolved.insert(key.0.clone());
            }
        }
    }
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
