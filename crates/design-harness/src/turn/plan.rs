use std::collections::{BTreeMap, BTreeSet};

use automation_state::{ActionSpec, ButtonRoute};
use serde_json::to_string;

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::protocol::{TurnBrief, TurnIntent};
use super::scope::{
    action_matches, overwrite_target_matches, requirement_satisfied, resource_ref_matches,
    ScopeAction, ScopeButtonRoute, ScopeInstanceRef, ScopeOverwriteTarget,
    ScopePostPanelButtonRoute, ScopeRequirement, ScopeResourceRef, ScopeRoleRef, ScopeTrigger,
};

const MAX_TURN_PLAN_REQUIREMENTS: usize = 32;
const NO_UNRESOLVED_ID: &str = "plan_no_unresolved_references";

pub(crate) fn normalize_turn_plan(
    draft: &Draft,
    brief: &TurnBrief,
    mut requirements: Vec<ScopeRequirement>,
) -> Result<Vec<ScopeRequirement>, StructuredError> {
    if brief.intent != TurnIntent::Build {
        return Err(plan_error(
            "TURN_PLAN_INTENT_UNSUPPORTED",
            "turn.plan.intent",
            "Turn plans currently support additive build turns only",
            "Use intent build for additive work or the existing modify workflow for edits and removals",
        ));
    }
    if !brief.requirements.is_empty() {
        return Err(plan_error(
            "TURN_PLAN_ALREADY_SET",
            "turn.plan",
            "The active turn already has an accepted plan",
            "Execute or finish the accepted plan instead of replacing it",
        ));
    }
    if requirements.is_empty() {
        return Err(plan_error(
            "EMPTY_TURN_PLAN",
            "turn.plan.requirements",
            "The turn plan has no requirements",
            "Declare at least one additive Draft requirement",
        ));
    }
    normalize_guard(&mut requirements)?;
    if requirements.len() > MAX_TURN_PLAN_REQUIREMENTS {
        return Err(plan_error(
            "TURN_PLAN_TOO_LARGE",
            "turn.plan.requirements",
            format!(
                "The turn plan has {} requirements but the maximum is {MAX_TURN_PLAN_REQUIREMENTS}",
                requirements.len()
            ),
            "Split the request into smaller human turns or reduce the plan",
        ));
    }
    validate_ids_and_identities(&requirements)?;
    validate_created_action_keys(draft, &requirements)?;
    validate_dependencies(draft, &requirements)?;
    validate_conflicts(draft, &requirements)?;
    validate_additive_action_order(draft, &requirements)?;
    if !requirements.iter().any(|requirement| {
        !matches!(requirement, ScopeRequirement::NoUnresolvedReferences { .. })
            && !requirement_satisfied(draft, requirement)
    }) {
        return Err(plan_error(
            "TURN_PLAN_NO_CHANGES",
            "turn.plan.requirements",
            "Every actionable turn plan requirement is already satisfied",
            "Use inspect for an unchanged Draft or declare an additive requirement that changes it",
        ));
    }
    Ok(requirements)
}

pub(crate) fn validate_final_planned_action_order(
    root: &Draft,
    candidate: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<(), StructuredError> {
    let expected = expected_action_orders(root, requirements)?;
    for (rule_key, actions) in expected {
        let Some(rule) = candidate
            .ruleset
            .rules
            .iter()
            .find(|rule| rule.key == rule_key)
        else {
            return Err(final_order_error(&rule_key));
        };
        if rule.actions.len() != actions.len()
            || !rule
                .actions
                .iter()
                .zip(&actions)
                .all(|(actual, expected)| expected.matches(actual))
        {
            return Err(final_order_error(&rule_key));
        }
    }
    Ok(())
}

fn normalize_guard(requirements: &mut Vec<ScopeRequirement>) -> Result<(), StructuredError> {
    let guards = requirements
        .iter()
        .enumerate()
        .filter_map(|(index, requirement)| {
            matches!(requirement, ScopeRequirement::NoUnresolvedReferences { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    match guards.as_slice() {
        [] => requirements.push(ScopeRequirement::NoUnresolvedReferences {
            id: NO_UNRESOLVED_ID.to_string(),
        }),
        [index] if *index + 1 == requirements.len() => {}
        [_] => {
            return Err(plan_error(
                "TURN_PLAN_GUARD_ORDER",
                "turn.plan.requirements",
                "NoUnresolvedReferences must be the final requirement",
                "Move the unresolved-reference guard to the end of the plan",
            ));
        }
        _ => {
            return Err(plan_error(
                "TURN_PLAN_GUARD_ORDER",
                "turn.plan.requirements",
                "The turn plan contains more than one unresolved-reference guard",
                "Keep exactly one NoUnresolvedReferences requirement at the end",
            ));
        }
    }
    if requirements.len() == 1 {
        return Err(plan_error(
            "TURN_PLAN_NOT_ACTIONABLE",
            "turn.plan.requirements",
            "The turn plan contains only a verification guard",
            "Declare at least one panel, button, modal, rule, or action requirement",
        ));
    }
    Ok(())
}

fn validate_ids_and_identities(requirements: &[ScopeRequirement]) -> Result<(), StructuredError> {
    let mut ids = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for requirement in requirements {
        let id = requirement.id();
        if id.trim().is_empty() {
            return Err(plan_error(
                "EMPTY_TURN_PLAN_REQUIREMENT_ID",
                "turn.plan.requirements.id",
                "A turn plan requirement has an empty id",
                "Give every requirement a stable non-empty id",
            ));
        }
        if !ids.insert(id.to_string()) {
            return Err(plan_error(
                "DUPLICATE_TURN_PLAN_REQUIREMENT_ID",
                format!("turn.plan.requirements.{id}"),
                format!("The turn plan repeats requirement id {id}"),
                "Use each requirement id exactly once",
            ));
        }
        validate_requirement_shape(requirement)?;
        let identity = requirement_identity(requirement);
        if !identities.insert(identity.clone()) {
            return Err(plan_error(
                "TURN_PLAN_DUPLICATE_IDENTITY",
                format!("turn.plan.requirements.{id}"),
                format!("The turn plan repeats operation identity {identity}"),
                "Keep one requirement for each stable Draft identity",
            ));
        }
    }
    Ok(())
}

fn validate_requirement_shape(requirement: &ScopeRequirement) -> Result<(), StructuredError> {
    let invalid_key = match requirement {
        ScopeRequirement::Panel { key, .. }
        | ScopeRequirement::Modal { key, .. }
        | ScopeRequirement::Rule { key, .. } => key.trim().is_empty(),
        ScopeRequirement::Button { panel_key, .. } => panel_key.trim().is_empty(),
        ScopeRequirement::Action {
            rule_key,
            action,
            minimum,
            ..
        } => {
            if *minimum == 0 {
                return Err(plan_error(
                    "TURN_PLAN_INVALID_MINIMUM",
                    format!("turn.plan.requirements.{}.minimum", requirement.id()),
                    "An action requirement has a zero target count",
                    "Set minimum to an absolute target count of at least one",
                ));
            }
            if action_stable_key(action).is_some() && *minimum != 1 {
                return Err(plan_error(
                    "TURN_PLAN_INVALID_MINIMUM",
                    format!("turn.plan.requirements.{}.minimum", requirement.id()),
                    "A keyed action requirement must have an absolute target count of one",
                    "Set minimum to one for keyed actions",
                ));
            }
            if matches!(
                action,
                ScopeAction::DeferEphemeral | ScopeAction::EditResponse { .. }
            ) && *minimum != 1
            {
                return Err(plan_error(
                    "TURN_PLAN_INVALID_MINIMUM",
                    format!("turn.plan.requirements.{}.minimum", requirement.id()),
                    "DeferEphemeral and EditResponse are singleton actions",
                    "Set minimum to one for singleton response lifecycle actions",
                ));
            }
            rule_key.trim().is_empty()
                || action_stable_key(action).is_some_and(|key| key.trim().is_empty())
        }
        ScopeRequirement::NoUnresolvedReferences { .. } => false,
    };
    if invalid_key {
        return Err(plan_error(
            "EMPTY_TURN_PLAN_TARGET",
            format!("turn.plan.requirements.{}", requirement.id()),
            "A turn plan requirement has an empty stable target",
            "Provide a non-empty stable key for every target",
        ));
    }
    Ok(())
}

fn validate_created_action_keys(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<(), StructuredError> {
    let mut keys = BTreeMap::<String, BTreeMap<String, &'static str>>::new();
    for rule in &draft.ruleset.rules {
        let rule_keys = keys.entry(rule.key.clone()).or_default();
        for action in &rule.actions {
            let Some((key, kind)) = action_spec_created_key(action) else {
                continue;
            };
            if let Some(existing) = rule_keys.insert(key.to_string(), kind) {
                return Err(created_key_error(&rule.key, key, existing, kind));
            }
        }
    }
    for requirement in requirements {
        let ScopeRequirement::Action {
            rule_key, action, ..
        } = requirement
        else {
            continue;
        };
        let Some((key, kind)) = scope_action_created_key(action) else {
            continue;
        };
        let exact_existing = draft
            .ruleset
            .rules
            .iter()
            .find(|rule| rule.key == *rule_key)
            .is_some_and(|rule| {
                rule.actions
                    .iter()
                    .any(|candidate| action_matches(candidate, action))
            });
        if exact_existing {
            continue;
        }
        let rule_keys = keys.entry(rule_key.clone()).or_default();
        if let Some(existing) = rule_keys.insert(key.to_string(), kind) {
            return Err(created_key_error(rule_key, key, existing, kind));
        }
    }
    Ok(())
}

fn action_spec_created_key(action: &ActionSpec) -> Option<(&str, &'static str)> {
    match action {
        ActionSpec::CreateRole { key, .. } => Some((key, "create_role")),
        ActionSpec::CreateChannel { key, .. } => Some((key, "create_channel")),
        ActionSpec::PostPanel { key, .. } => Some((key, "post_panel")),
        ActionSpec::RegisterInstance { key, .. } => Some((key, "register_instance")),
        _ => None,
    }
}

fn scope_action_created_key(action: &ScopeAction) -> Option<(&str, &'static str)> {
    match action {
        ScopeAction::CreateRole { key, .. } => Some((key, "create_role")),
        ScopeAction::CreateChannel { key, .. } => Some((key, "create_channel")),
        ScopeAction::PostPanel { key, .. } => Some((key, "post_panel")),
        ScopeAction::RegisterInstance { key, .. } => Some((key, "register_instance")),
        _ => None,
    }
}

fn created_key_error(
    rule_key: &str,
    key: &str,
    existing: &str,
    requested: &str,
) -> StructuredError {
    plan_error(
        "TURN_PLAN_CREATED_KEY_CONFLICT",
        format!("turn.plan.rules.{rule_key}.created.{key}"),
        format!(
            "Created action key {key} in rule {rule_key} is shared by {existing} and {requested}"
        ),
        "Use one unique created action key across roles, channels, posted panels, and instances in each rule",
    )
}

fn validate_dependencies(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<(), StructuredError> {
    let mut panels = draft
        .ruleset
        .panels
        .iter()
        .map(|panel| panel.key.clone())
        .collect::<BTreeSet<_>>();
    let mut modals = draft
        .ruleset
        .modals
        .iter()
        .map(|modal| modal.key.clone())
        .collect::<BTreeSet<_>>();
    let mut rules = draft
        .ruleset
        .rules
        .iter()
        .map(|rule| rule.key.clone())
        .collect::<BTreeSet<_>>();
    let mut button_components = draft
        .ruleset
        .panels
        .iter()
        .flat_map(|panel| panel.buttons.iter())
        .filter_map(|button| match &button.route {
            ButtonRoute::Static { key } => Some(key.clone()),
            ButtonRoute::InstanceAction { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let mut resources = draft_resources(draft);
    let registrations = planned_registrations(requirements)?;

    for (index, requirement) in requirements.iter().enumerate() {
        match requirement {
            ScopeRequirement::Panel { key, .. } => {
                panels.insert(key.clone());
            }
            ScopeRequirement::Button {
                panel_key, route, ..
            } => {
                require_symbol(
                    panels.contains(panel_key),
                    requirement,
                    format!("panel {panel_key}"),
                )?;
                if let ScopeButtonRoute::Static { key } = route {
                    button_components.insert(key.clone());
                }
            }
            ScopeRequirement::Modal { key, .. } => {
                modals.insert(key.clone());
            }
            ScopeRequirement::Rule { key, trigger, .. } => {
                match trigger {
                    ScopeTrigger::ButtonClick { component } => require_symbol(
                        button_components.contains(component),
                        requirement,
                        format!("button component {component}"),
                    )?,
                    ScopeTrigger::ModalSubmit { modal } => require_symbol(
                        modals.contains(modal),
                        requirement,
                        format!("modal {modal}"),
                    )?,
                    ScopeTrigger::InstanceAction { .. } => {}
                }
                rules.insert(key.clone());
            }
            ScopeRequirement::Action {
                rule_key, action, ..
            } => {
                require_symbol(
                    rules.contains(rule_key),
                    requirement,
                    format!("rule {rule_key}"),
                )?;
                validate_action_dependencies(
                    requirement,
                    index,
                    rule_key,
                    action,
                    &modals,
                    &resources,
                    &registrations,
                )?;
                record_action_resource(&mut resources, rule_key, action);
            }
            ScopeRequirement::NoUnresolvedReferences { .. } => {}
        }
    }
    Ok(())
}

fn validate_action_dependencies(
    requirement: &ScopeRequirement,
    requirement_index: usize,
    rule_key: &str,
    action: &ScopeAction,
    modals: &BTreeSet<String>,
    resources: &BTreeMap<String, RuleResources>,
    registrations: &BTreeMap<String, PlannedRegistration>,
) -> Result<(), StructuredError> {
    let empty = RuleResources::default();
    let available = resources.get(rule_key).unwrap_or(&empty);
    match action {
        ScopeAction::OpenModal { modal } => require_symbol(
            modals.contains(modal),
            requirement,
            format!("modal {modal}"),
        )?,
        ScopeAction::GrantRole { role, .. } => require_role(requirement, role, available)?,
        ScopeAction::UpsertOverwrite {
            channel, target, ..
        } => {
            require_channel(requirement, channel, available)?;
            if let ScopeOverwriteTarget::Role { role } = target {
                require_role(requirement, role, available)?;
            }
        }
        ScopeAction::PostPanel {
            channel, buttons, ..
        } => {
            require_channel(requirement, channel, available)?;
            for button in buttons {
                if let ScopePostPanelButtonRoute::InstanceAction { instance, .. } = &button.route {
                    require_post_panel_instance(
                        requirement,
                        requirement_index,
                        rule_key,
                        instance,
                        registrations,
                    )?;
                }
            }
        }
        ScopeAction::RegisterInstance { resources, .. } => {
            validate_manifest(requirement, resources, available)?;
        }
        ScopeAction::TeardownInstance { instance } => {
            if !matches!(instance, ScopeInstanceRef::Event) {
                return Err(dependency_error(
                    requirement,
                    "teardown_instance requires the current event instance",
                ));
            }
        }
        ScopeAction::RespondEphemeral { .. }
        | ScopeAction::CreateChannel { .. }
        | ScopeAction::CreateRole { .. }
        | ScopeAction::DeferEphemeral
        | ScopeAction::EditResponse { .. } => {}
    }
    Ok(())
}

fn validate_manifest(
    requirement: &ScopeRequirement,
    manifest: &super::scope::ScopeInstanceResources,
    available: &RuleResources,
) -> Result<(), StructuredError> {
    let roles = manifest
        .roles
        .iter()
        .map(|entry| entry.created.clone())
        .collect::<BTreeSet<_>>();
    let channels = manifest
        .channels
        .iter()
        .map(|entry| entry.created.clone())
        .collect::<BTreeSet<_>>();
    let messages = manifest
        .messages
        .iter()
        .map(|entry| entry.created.clone())
        .collect::<BTreeSet<_>>();
    let unique_aliases = manifest
        .roles
        .iter()
        .chain(&manifest.channels)
        .chain(&manifest.messages)
        .map(|entry| entry.alias.as_str())
        .collect::<BTreeSet<_>>();
    let total = manifest.roles.len() + manifest.channels.len() + manifest.messages.len();
    if unique_aliases.len() != total
        || roles != available.roles
        || channels != available.channels
        || messages != available.messages
    {
        return Err(dependency_error(
            requirement,
            "register_instance manifest must cover each created role, channel, and message exactly once with unique aliases",
        ));
    }
    Ok(())
}

fn validate_conflicts(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<(), StructuredError> {
    for requirement in requirements {
        if requirement_satisfied(draft, requirement) {
            continue;
        }
        let conflict = match requirement {
            ScopeRequirement::Panel { key, .. } => {
                draft.ruleset.panels.iter().any(|panel| panel.key == *key)
            }
            ScopeRequirement::Button {
                panel_key, route, ..
            } => draft
                .ruleset
                .panels
                .iter()
                .find(|panel| panel.key == *panel_key)
                .is_some_and(|panel| {
                    panel
                        .buttons
                        .iter()
                        .any(|button| button_identity_matches(&button.route, route))
                }),
            ScopeRequirement::Modal { key, .. } => {
                draft.ruleset.modals.iter().any(|modal| modal.key == *key)
            }
            ScopeRequirement::Rule { key, .. } => {
                draft.ruleset.rules.iter().any(|rule| rule.key == *key)
            }
            ScopeRequirement::Action {
                rule_key, action, ..
            } => draft
                .ruleset
                .rules
                .iter()
                .find(|rule| rule.key == *rule_key)
                .is_some_and(|rule| {
                    rule.actions.iter().any(|candidate| {
                        action_identity_matches(candidate, action)
                            && !action_matches(candidate, action)
                    })
                }),
            ScopeRequirement::NoUnresolvedReferences { .. } => false,
        };
        if conflict {
            return Err(plan_error(
                "TURN_PLAN_CONFLICT",
                format!("turn.plan.requirements.{}", requirement.id()),
                format!(
                    "Draft identity {} already exists with different semantics",
                    requirement_identity(requirement)
                ),
                "Use the existing modify workflow instead of adding a duplicate identity",
            ));
        }
    }
    Ok(())
}

fn validate_additive_action_order(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<(), StructuredError> {
    expected_action_orders(draft, requirements).map(|_| ())
}

#[derive(Clone)]
enum ExpectedAction {
    Existing(ActionSpec),
    Planned(ScopeAction),
}

impl ExpectedAction {
    fn matches(&self, actual: &ActionSpec) -> bool {
        match self {
            Self::Existing(expected) => actual == expected,
            Self::Planned(expected) => action_matches(actual, expected),
        }
    }
}

#[derive(Clone, Copy)]
enum ActionPlacement {
    Defer,
    Edit,
    Register,
    Regular,
}

#[derive(Clone)]
struct OrderedAction {
    token: String,
    expected: ExpectedAction,
    placement: ActionPlacement,
}

fn expected_action_orders(
    draft: &Draft,
    requirements: &[ScopeRequirement],
) -> Result<BTreeMap<String, Vec<ExpectedAction>>, StructuredError> {
    let mut declared = BTreeMap::<String, Vec<OrderedAction>>::new();
    let mut simulated = BTreeMap::<String, Vec<OrderedAction>>::new();
    for rule in &draft.ruleset.rules {
        let actions = rule
            .actions
            .iter()
            .enumerate()
            .map(|(index, action)| OrderedAction {
                token: format!("root:{}:{index}", rule.key),
                expected: ExpectedAction::Existing(action.clone()),
                placement: action_spec_placement(action),
            })
            .collect::<Vec<_>>();
        declared.insert(rule.key.clone(), actions.clone());
        simulated.insert(rule.key.clone(), actions);
    }
    let mut rules_with_missing_actions = BTreeSet::new();
    let mut last_exact_positions = BTreeMap::<String, usize>::new();
    for requirement in requirements {
        let ScopeRequirement::Action {
            rule_key,
            action,
            minimum,
            ..
        } = requirement
        else {
            continue;
        };
        if requirement_satisfied(draft, requirement) {
            if rules_with_missing_actions.contains(rule_key) {
                return Err(existing_action_order_error(
                    requirement,
                    format!(
                        "An existing exact action in rule {rule_key} appears after a newly appended action"
                    ),
                ));
            }
            let previous = last_exact_positions.get(rule_key).copied();
            let matching = draft
                .ruleset
                .rules
                .iter()
                .find(|rule| rule.key == *rule_key)
                .into_iter()
                .flat_map(|rule| rule.actions.iter().enumerate())
                .filter_map(|(index, candidate)| {
                    (previous.is_none_or(|previous| index > previous)
                        && action_matches(candidate, action))
                    .then_some(index)
                })
                .take(*minimum)
                .collect::<Vec<_>>();
            if matching.len() != *minimum {
                return Err(existing_action_order_error(
                    requirement,
                    format!(
                        "Existing exact actions in rule {rule_key} are declared out of their Draft order"
                    ),
                ));
            }
            last_exact_positions.insert(rule_key.clone(), *matching.last().unwrap());
        } else {
            rules_with_missing_actions.insert(rule_key.clone());
            let exact_count = draft
                .ruleset
                .rules
                .iter()
                .find(|rule| rule.key == *rule_key)
                .map_or(0, |rule| {
                    rule.actions
                        .iter()
                        .filter(|candidate| action_matches(candidate, action))
                        .count()
                });
            let deficit = minimum.saturating_sub(exact_count);
            for occurrence in 0..deficit {
                let item = OrderedAction {
                    token: format!("plan:{}:{occurrence}", requirement.id()),
                    expected: ExpectedAction::Planned(action.clone()),
                    placement: scope_action_placement(action),
                };
                declared
                    .entry(rule_key.clone())
                    .or_default()
                    .push(item.clone());
                insert_ordered_action(simulated.entry(rule_key.clone()).or_default(), item);
                let expected_tokens = declared[rule_key]
                    .iter()
                    .map(|item| item.token.as_str())
                    .collect::<Vec<_>>();
                let actual_tokens = simulated[rule_key]
                    .iter()
                    .map(|item| item.token.as_str())
                    .collect::<Vec<_>>();
                if actual_tokens != expected_tokens {
                    return Err(action_order_error(
                        requirement,
                        format!(
                            "The existing design tools cannot preserve the declared additive action order in rule {rule_key}"
                        ),
                    ));
                }
            }
        }
    }
    Ok(declared
        .into_iter()
        .map(|(rule, actions)| {
            (
                rule,
                actions.into_iter().map(|action| action.expected).collect(),
            )
        })
        .collect())
}

fn insert_ordered_action(actions: &mut Vec<OrderedAction>, action: OrderedAction) {
    let index = match action.placement {
        ActionPlacement::Defer => 0,
        ActionPlacement::Edit => actions.len(),
        ActionPlacement::Register => actions
            .iter()
            .position(|candidate| matches!(candidate.placement, ActionPlacement::Edit))
            .unwrap_or(actions.len()),
        ActionPlacement::Regular => actions
            .iter()
            .position(|candidate| {
                matches!(
                    candidate.placement,
                    ActionPlacement::Register | ActionPlacement::Edit
                )
            })
            .unwrap_or(actions.len()),
    };
    actions.insert(index, action);
}

fn action_spec_placement(action: &ActionSpec) -> ActionPlacement {
    match action {
        ActionSpec::DeferEphemeral => ActionPlacement::Defer,
        ActionSpec::EditResponse { .. } => ActionPlacement::Edit,
        ActionSpec::RegisterInstance { .. } => ActionPlacement::Register,
        _ => ActionPlacement::Regular,
    }
}

fn scope_action_placement(action: &ScopeAction) -> ActionPlacement {
    match action {
        ScopeAction::DeferEphemeral => ActionPlacement::Defer,
        ScopeAction::EditResponse { .. } => ActionPlacement::Edit,
        ScopeAction::RegisterInstance { .. } => ActionPlacement::Register,
        _ => ActionPlacement::Regular,
    }
}

fn action_order_error(requirement: &ScopeRequirement, message: String) -> StructuredError {
    plan_error(
        "TURN_PLAN_ACTION_ORDER",
        format!("turn.plan.requirements.{}", requirement.id()),
        message,
        "Keep existing exact actions first and declare new actions in the order preserved by Defer, RegisterInstance, and EditResponse placement rules",
    )
}

fn existing_action_order_error(requirement: &ScopeRequirement, message: String) -> StructuredError {
    plan_error(
        "TURN_PLAN_EXISTING_ACTION_ORDER",
        format!("turn.plan.requirements.{}", requirement.id()),
        message,
        "Declare existing exact actions in their current Draft order before newly appended actions",
    )
}

fn final_order_error(rule_key: &str) -> StructuredError {
    plan_error(
        "PLAN_FINAL_ACTION_ORDER_MISMATCH",
        format!("turn.plan.rules.{rule_key}.actions"),
        format!(
            "The final action order in rule {rule_key} differs from the accepted additive plan"
        ),
        "Discard the candidate and keep the pre-plan Draft root",
    )
}

#[derive(Default)]
struct RuleResources {
    roles: BTreeSet<String>,
    channels: BTreeSet<String>,
    messages: BTreeSet<String>,
}

fn draft_resources(draft: &Draft) -> BTreeMap<String, RuleResources> {
    draft
        .ruleset
        .rules
        .iter()
        .map(|rule| {
            let mut resources = RuleResources::default();
            for action in &rule.actions {
                match action {
                    ActionSpec::CreateRole { key, .. } => {
                        resources.roles.insert(key.clone());
                    }
                    ActionSpec::CreateChannel { key, .. } => {
                        resources.channels.insert(key.clone());
                    }
                    ActionSpec::PostPanel { key, .. } => {
                        resources.messages.insert(key.clone());
                    }
                    _ => {}
                }
            }
            (rule.key.clone(), resources)
        })
        .collect()
}

#[derive(Clone)]
struct PlannedRegistration {
    key: String,
    index: usize,
}

fn planned_registrations(
    requirements: &[ScopeRequirement],
) -> Result<BTreeMap<String, PlannedRegistration>, StructuredError> {
    let mut registrations = BTreeMap::<String, PlannedRegistration>::new();
    for (index, requirement) in requirements.iter().enumerate() {
        if let ScopeRequirement::Action {
            rule_key,
            action: ScopeAction::RegisterInstance { key, .. },
            ..
        } = requirement
        {
            if registrations
                .insert(
                    rule_key.clone(),
                    PlannedRegistration {
                        key: key.clone(),
                        index,
                    },
                )
                .is_some()
            {
                return Err(plan_error(
                    "TURN_PLAN_MULTIPLE_REGISTRATIONS",
                    format!("turn.plan.requirements.{}", requirement.id()),
                    format!("Rule {rule_key} has more than one planned instance registration"),
                    "Keep one RegisterInstance requirement per creation rule",
                ));
            }
        }
    }
    Ok(registrations)
}

fn record_action_resource(
    resources: &mut BTreeMap<String, RuleResources>,
    rule_key: &str,
    action: &ScopeAction,
) {
    let resources = resources.entry(rule_key.to_string()).or_default();
    match action {
        ScopeAction::CreateRole { key, .. } => {
            resources.roles.insert(key.clone());
        }
        ScopeAction::CreateChannel { key, .. } => {
            resources.channels.insert(key.clone());
        }
        ScopeAction::PostPanel { key, .. } => {
            resources.messages.insert(key.clone());
        }
        _ => {}
    }
}

fn require_channel(
    requirement: &ScopeRequirement,
    reference: &ScopeResourceRef,
    resources: &RuleResources,
) -> Result<(), StructuredError> {
    match reference {
        ScopeResourceRef::Created { name } => require_symbol(
            resources.channels.contains(name),
            requirement,
            format!("created channel {name}"),
        ),
        ScopeResourceRef::Existing { name } => require_symbol(
            !name.trim().is_empty(),
            requirement,
            "a non-empty existing channel".to_string(),
        ),
    }
}

fn require_role(
    requirement: &ScopeRequirement,
    reference: &ScopeRoleRef,
    resources: &RuleResources,
) -> Result<(), StructuredError> {
    match reference {
        ScopeRoleRef::Created { name } => require_symbol(
            resources.roles.contains(name),
            requirement,
            format!("created role {name}"),
        ),
        ScopeRoleRef::Existing { name } => require_symbol(
            !name.trim().is_empty(),
            requirement,
            "a non-empty existing role".to_string(),
        ),
        ScopeRoleRef::Instance { .. } => Err(unsupported_reference(
            requirement,
            "instance role references are not supported by additive plan compilation",
        )),
    }
}

fn require_post_panel_instance(
    requirement: &ScopeRequirement,
    requirement_index: usize,
    rule_key: &str,
    reference: &ScopeInstanceRef,
    registrations: &BTreeMap<String, PlannedRegistration>,
) -> Result<(), StructuredError> {
    match reference {
        ScopeInstanceRef::Event => Err(unsupported_reference(
            requirement,
            "post_panel instance actions require a created instance registration",
        )),
        ScopeInstanceRef::Created { name } => {
            let planned = registrations.get(rule_key);
            require_symbol(
                planned.is_some_and(|registration| {
                    registration.key == *name && registration.index > requirement_index
                }),
                requirement,
                format!("later instance registration {name} in rule {rule_key}"),
            )
        }
    }
}

fn unsupported_reference(requirement: &ScopeRequirement, message: &str) -> StructuredError {
    plan_error(
        "TURN_PLAN_UNSUPPORTED_REFERENCE",
        format!("turn.plan.requirements.{}", requirement.id()),
        message,
        "Use a created or existing resource reference supported by the existing design tools",
    )
}

fn require_symbol(
    exists: bool,
    requirement: &ScopeRequirement,
    target: String,
) -> Result<(), StructuredError> {
    if exists {
        return Ok(());
    }
    Err(dependency_error(
        requirement,
        format!("Required prior target {target} does not exist"),
    ))
}

fn dependency_error(requirement: &ScopeRequirement, message: impl Into<String>) -> StructuredError {
    plan_error(
        "TURN_PLAN_DEPENDENCY_MISSING",
        format!("turn.plan.requirements.{}", requirement.id()),
        message,
        "Order the target requirement before this operation and include every referenced resource",
    )
}

fn requirement_identity(requirement: &ScopeRequirement) -> String {
    match requirement {
        ScopeRequirement::Panel { key, .. } => format!("panel:{key}"),
        ScopeRequirement::Button {
            panel_key, route, ..
        } => format!("button:{panel_key}:{}", button_route_identity(route)),
        ScopeRequirement::Modal { key, .. } => format!("modal:{key}"),
        ScopeRequirement::Rule { key, .. } => format!("rule:{key}"),
        ScopeRequirement::Action {
            rule_key, action, ..
        } => format!("action:{rule_key}:{}", scope_action_identity(action)),
        ScopeRequirement::NoUnresolvedReferences { .. } => "guard:no_unresolved".to_string(),
    }
}

fn button_route_identity(route: &ScopeButtonRoute) -> String {
    match route {
        ScopeButtonRoute::Static { key } => format!("static:{key}"),
        ScopeButtonRoute::InstanceAction { action } => format!("instance_action:{action}"),
    }
}

fn scope_action_identity(action: &ScopeAction) -> String {
    if let Some(key) = action_stable_key(action) {
        return format!("{:?}:{key}", action.kind());
    }
    match action {
        ScopeAction::DeferEphemeral => "defer_ephemeral".to_string(),
        ScopeAction::EditResponse { .. } => "edit_response".to_string(),
        ScopeAction::RespondEphemeral { .. } => "respond_ephemeral".to_string(),
        ScopeAction::OpenModal { modal } => format!("open_modal:{modal}"),
        ScopeAction::GrantRole { role, target } => {
            format!("grant_role:{}:{target:?}", json_identity(role))
        }
        ScopeAction::UpsertOverwrite {
            channel, target, ..
        } => format!(
            "upsert_overwrite:{}:{}",
            json_identity(channel),
            json_identity(target)
        ),
        ScopeAction::TeardownInstance { instance } => {
            format!("teardown_instance:{}", json_identity(instance))
        }
        ScopeAction::CreateChannel { .. }
        | ScopeAction::CreateRole { .. }
        | ScopeAction::PostPanel { .. }
        | ScopeAction::RegisterInstance { .. } => unreachable!(),
    }
}

fn action_stable_key(action: &ScopeAction) -> Option<&str> {
    match action {
        ScopeAction::CreateChannel { key, .. }
        | ScopeAction::CreateRole { key, .. }
        | ScopeAction::PostPanel { key, .. }
        | ScopeAction::RegisterInstance { key, .. } => Some(key),
        _ => None,
    }
}

fn json_identity<T: serde::Serialize>(value: &T) -> String {
    to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn button_identity_matches(actual: &ButtonRoute, expected: &ScopeButtonRoute) -> bool {
    match (actual, expected) {
        (ButtonRoute::Static { key }, ScopeButtonRoute::Static { key: expected }) => {
            key == expected
        }
        (
            ButtonRoute::InstanceAction { action, .. },
            ScopeButtonRoute::InstanceAction { action: expected },
        ) => action == expected,
        _ => false,
    }
}

fn action_identity_matches(actual: &ActionSpec, expected: &ScopeAction) -> bool {
    if action_matches(actual, expected) {
        return true;
    }
    match (actual, expected) {
        (ActionSpec::CreateRole { key, .. }, ScopeAction::CreateRole { key: expected, .. })
        | (
            ActionSpec::CreateChannel { key, .. },
            ScopeAction::CreateChannel { key: expected, .. },
        )
        | (ActionSpec::PostPanel { key, .. }, ScopeAction::PostPanel { key: expected, .. }) => {
            key == expected
        }
        (ActionSpec::RegisterInstance { .. }, ScopeAction::RegisterInstance { .. }) => true,
        (
            ActionSpec::UpsertOverwrite {
                channel, target, ..
            },
            ScopeAction::UpsertOverwrite {
                channel: expected_channel,
                target: expected_target,
                ..
            },
        ) => {
            resource_ref_matches(channel, expected_channel)
                && overwrite_target_matches(target, expected_target)
        }
        (ActionSpec::DeferEphemeral, ScopeAction::DeferEphemeral)
        | (ActionSpec::EditResponse { .. }, ScopeAction::EditResponse { .. })
        | (ActionSpec::RespondEphemeral { .. }, ScopeAction::RespondEphemeral { .. }) => true,
        (ActionSpec::OpenModal { modal }, ScopeAction::OpenModal { modal: expected }) => {
            modal == expected
        }
        _ => false,
    }
}

fn plan_error(
    code: &str,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}

#[cfg(test)]
mod tests {
    use automation_state::{InteractionRule, InteractionRuleSet, TriggerSpec};
    use serde_json::json;

    use super::*;
    use crate::turn::{RequestedOutcome, SimulationProfile, TurnVerification};

    fn brief() -> TurnBrief {
        TurnBrief {
            intent: TurnIntent::Build,
            objective: "Build the requested automation".to_string(),
            requested_outcome: RequestedOutcome::DraftUpdate,
            requirements: Vec::new(),
            assumptions: Vec::new(),
            blocking_decisions: Vec::new(),
            verification: TurnVerification {
                validate: false,
                simulation: SimulationProfile::None,
            },
        }
    }

    fn requirement(value: serde_json::Value) -> ScopeRequirement {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn plan_normalizes_the_final_reference_guard_and_maps_tools() {
        let requirements = vec![
            requirement(json!({
                "kind":"panel",
                "id":"panel",
                "key":"study_panel",
                "channel":"study_hub",
                "content":"Create a study room"
            })),
            requirement(json!({
                "kind":"button",
                "id":"button",
                "panel_key":"study_panel",
                "label":"Create room",
                "route":{"kind":"static","key":"create_study_room"}
            })),
            requirement(json!({
                "kind":"modal",
                "id":"modal",
                "key":"study_modal",
                "title":"Create study room",
                "fields":[]
            })),
            requirement(json!({
                "kind":"rule",
                "id":"rule",
                "key":"open_modal",
                "trigger":{"kind":"button_click","component":"create_study_room"}
            })),
            requirement(json!({
                "kind":"action",
                "id":"open",
                "rule_key":"open_modal",
                "action":{"kind":"open_modal","modal":"study_modal"},
                "minimum":1
            })),
        ];

        let plan = normalize_turn_plan(&Draft::new(), &brief(), requirements).unwrap();

        assert_eq!(plan.len(), 6);
        assert!(matches!(
            plan.last(),
            Some(ScopeRequirement::NoUnresolvedReferences { id }) if id == NO_UNRESOLVED_ID
        ));
    }

    #[test]
    fn plan_rejects_invalid_shape_order_and_dependencies() {
        let empty = normalize_turn_plan(&Draft::new(), &brief(), Vec::new()).unwrap_err();
        assert_eq!(empty.code, "EMPTY_TURN_PLAN");

        let duplicate = vec![
            requirement(
                json!({"kind":"panel","id":"same","key":"one","channel":"c","content":"1"}),
            ),
            requirement(json!({"kind":"modal","id":"same","key":"two","title":"2","fields":[]})),
        ];
        assert_eq!(
            normalize_turn_plan(&Draft::new(), &brief(), duplicate)
                .unwrap_err()
                .code,
            "DUPLICATE_TURN_PLAN_REQUIREMENT_ID"
        );

        let missing_target = vec![requirement(json!({
            "kind":"action",
            "id":"action",
            "rule_key":"missing",
            "action":{"kind":"defer_ephemeral"},
            "minimum":1
        }))];
        assert_eq!(
            normalize_turn_plan(&Draft::new(), &brief(), missing_target)
                .unwrap_err()
                .code,
            "TURN_PLAN_DEPENDENCY_MISSING"
        );

        let guard_first = vec![
            requirement(json!({"kind":"no_unresolved_references","id":"refs"})),
            requirement(
                json!({"kind":"panel","id":"panel","key":"one","channel":"c","content":"1"}),
            ),
        ];
        assert_eq!(
            normalize_turn_plan(&Draft::new(), &brief(), guard_first)
                .unwrap_err()
                .code,
            "TURN_PLAN_GUARD_ORDER"
        );
    }

    #[test]
    fn plan_rejects_divergent_stable_identity_without_duplicate_addition() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[{"key":"study_panel","channel":"study_hub","content":"Old","buttons":[]}],
            "modals":[],
            "rules":[]
        }))
        .unwrap();
        let plan = vec![requirement(json!({
            "kind":"panel",
            "id":"panel",
            "key":"study_panel",
            "channel":"study_hub",
            "content":"New"
        }))];

        let error = normalize_turn_plan(&draft, &brief(), plan).unwrap_err();

        assert_eq!(error.code, "TURN_PLAN_CONFLICT");
    }

    #[test]
    fn plan_rejects_an_all_satisfied_build() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[{"key":"study_panel","channel":"study_hub","content":"Ready","buttons":[]}],
            "modals":[],
            "rules":[]
        }))
        .unwrap();
        let requirements = vec![requirement(json!({
            "kind":"panel",
            "id":"panel",
            "key":"study_panel",
            "channel":"study_hub",
            "content":"Ready"
        }))];

        let error = normalize_turn_plan(&draft, &brief(), requirements).unwrap_err();

        assert_eq!(error.code, "TURN_PLAN_NO_CHANGES");
    }

    #[test]
    fn finalize_plan_accepts_forward_instance_registration_and_exact_manifest() {
        let mut draft = Draft::new();
        draft.ruleset = InteractionRuleSet {
            version: 1,
            panels: Vec::new(),
            modals: Vec::new(),
            rules: vec![InteractionRule {
                key: "submit_room".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "submit".to_string(),
                },
                actions: vec![
                    ActionSpec::CreateRole {
                        key: "member_role".to_string(),
                        name: "Members".to_string(),
                    },
                    ActionSpec::CreateChannel {
                        key: "room_channel".to_string(),
                        name: "Room".to_string(),
                    },
                ],
            }],
        };
        let requirements = vec![
            requirement(json!({
                "kind":"action",
                "id":"welcome",
                "rule_key":"submit_room",
                "action":{
                    "kind":"post_panel",
                    "key":"welcome_panel",
                    "channel":{"kind":"created","name":"room_channel"},
                    "content":"Welcome",
                    "buttons":[{"label":"Close","route":{"kind":"instance_action","instance":{"kind":"created","name":"study_instance"},"action":"close"}}]
                },
                "minimum":1
            })),
            requirement(json!({
                "kind":"action",
                "id":"register",
                "rule_key":"submit_room",
                "action":{
                    "kind":"register_instance",
                    "key":"study_instance",
                    "instance_kind":"study_room",
                    "resources":{
                        "roles":[{"alias":"member_role","created":"member_role"}],
                        "channels":[{"alias":"room_channel","created":"room_channel"}],
                        "messages":[{"alias":"welcome_panel","created":"welcome_panel"}]
                    }
                },
                "minimum":1
            })),
            requirement(json!({
                "kind":"action",
                "id":"edit",
                "rule_key":"submit_room",
                "action":{"kind":"edit_response","content":"Created"},
                "minimum":1
            })),
        ];

        let plan = normalize_turn_plan(&draft, &brief(), requirements).unwrap();

        assert_eq!(plan.len(), 4);
        assert!(matches!(
            &plan[0],
            ScopeRequirement::Action {
                action: ScopeAction::PostPanel { .. },
                ..
            }
        ));
        assert!(matches!(
            &plan[1],
            ScopeRequirement::Action {
                action: ScopeAction::RegisterInstance { .. },
                ..
            }
        ));
    }

    #[test]
    fn plan_rejects_references_the_existing_add_tools_cannot_compile() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[
                    {"type":"create_role","key":"member_role","name":"Members"},
                    {"type":"create_channel","key":"room_channel","name":"Room"}
                ]
            }]
        }))
        .unwrap();
        let instance_role = vec![requirement(json!({
            "kind":"action",
            "id":"grant",
            "rule_key":"room",
            "action":{
                "kind":"grant_role",
                "role":{"kind":"instance","instance":{"kind":"event"},"alias":"member_role"},
                "target":"actor"
            },
            "minimum":1
        }))];
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), instance_role)
                .unwrap_err()
                .code,
            "TURN_PLAN_UNSUPPORTED_REFERENCE"
        );

        let event_panel = vec![
            requirement(json!({
                "kind":"action",
                "id":"panel",
                "rule_key":"room",
                "action":{
                    "kind":"post_panel",
                    "key":"room_panel",
                    "channel":{"kind":"created","name":"room_channel"},
                    "content":"Room",
                    "buttons":[{"label":"Join","route":{"kind":"instance_action","instance":{"kind":"event"},"action":"join"}}]
                },
                "minimum":1
            })),
            requirement(json!({
                "kind":"action",
                "id":"register",
                "rule_key":"room",
                "action":{
                    "kind":"register_instance",
                    "key":"room_instance",
                    "instance_kind":"room",
                    "resources":{
                        "roles":[{"alias":"member_role","created":"member_role"}],
                        "channels":[{"alias":"room_channel","created":"room_channel"}],
                        "messages":[{"alias":"room_panel","created":"room_panel"}]
                    }
                },
                "minimum":1
            })),
        ];
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), event_panel)
                .unwrap_err()
                .code,
            "TURN_PLAN_UNSUPPORTED_REFERENCE"
        );
    }

    #[test]
    fn plan_requires_one_later_registration_for_pending_panels() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[
                    {"type":"create_channel","key":"room_channel","name":"Room"},
                    {
                        "type":"register_instance",
                        "key":"existing_instance",
                        "kind":"room",
                        "resources":{"roles":{},"channels":{"room":{"created":"room_channel"}},"messages":{}}
                    }
                ]
            }]
        }))
        .unwrap();
        let panel_only = vec![requirement(json!({
            "kind":"action",
            "id":"panel",
            "rule_key":"room",
            "action":{
                "kind":"post_panel",
                "key":"room_panel",
                "channel":{"kind":"created","name":"room_channel"},
                "content":"Room",
                "buttons":[{"label":"Close","route":{"kind":"instance_action","instance":{"kind":"created","name":"existing_instance"},"action":"close"}}]
            },
            "minimum":1
        }))];
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), panel_only)
                .unwrap_err()
                .code,
            "TURN_PLAN_DEPENDENCY_MISSING"
        );

        let two_registrations = vec![
            requirement(json!({
                "kind":"action",
                "id":"first",
                "rule_key":"room",
                "action":{
                    "kind":"register_instance",
                    "key":"first_instance",
                    "instance_kind":"room",
                    "resources":{"roles":[],"channels":[{"alias":"room","created":"room_channel"}],"messages":[]}
                },
                "minimum":1
            })),
            requirement(json!({
                "kind":"action",
                "id":"second",
                "rule_key":"room",
                "action":{
                    "kind":"register_instance",
                    "key":"second_instance",
                    "instance_kind":"room",
                    "resources":{"roles":[],"channels":[{"alias":"room","created":"room_channel"}],"messages":[]}
                },
                "minimum":1
            })),
        ];
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), two_registrations)
                .unwrap_err()
                .code,
            "TURN_PLAN_MULTIPLE_REGISTRATIONS"
        );
    }

    #[test]
    fn plan_rejects_a_divergent_overwrite_with_the_same_target_identity() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[
                    {"type":"create_channel","key":"room_channel","name":"Room"},
                    {
                        "type":"upsert_overwrite",
                        "channel":{"created":"room_channel"},
                        "target":"everyone",
                        "allow":"1024",
                        "deny":"0"
                    }
                ]
            }]
        }))
        .unwrap();
        let requirements = vec![requirement(json!({
            "kind":"action",
            "id":"overwrite",
            "rule_key":"room",
            "action":{
                "kind":"upsert_overwrite",
                "channel":{"kind":"created","name":"room_channel"},
                "target":{"kind":"everyone"},
                "allow":["send_messages"],
                "deny":[]
            },
            "minimum":1
        }))];

        let error = normalize_turn_plan(&draft, &brief(), requirements).unwrap_err();

        assert_eq!(error.code, "TURN_PLAN_CONFLICT");
    }

    #[test]
    fn repeatable_exact_action_uses_an_absolute_minimum_without_conflict() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[
                    {"type":"create_channel","key":"room_channel","name":"Room"},
                    {
                        "type":"upsert_overwrite",
                        "channel":{"created":"room_channel"},
                        "target":"everyone",
                        "allow":"0",
                        "deny":"1024"
                    }
                ]
            }]
        }))
        .unwrap();
        let requirements = vec![requirement(json!({
            "kind":"action",
            "id":"overwrite",
            "rule_key":"room",
            "action":{
                "kind":"upsert_overwrite",
                "channel":{"kind":"created","name":"room_channel"},
                "target":{"kind":"everyone"},
                "allow":[],
                "deny":["view_channel"]
            },
            "minimum":2
        }))];

        let plan = normalize_turn_plan(&draft, &brief(), requirements).unwrap();

        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn response_lifecycle_singletons_reject_multiple_absolute_targets() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[
                    {"type":"defer_ephemeral"},
                    {"type":"edit_response","content":"Old"}
                ]
            }]
        }))
        .unwrap();
        for action in [
            json!({"kind":"defer_ephemeral"}),
            json!({"kind":"edit_response","content":"Old"}),
        ] {
            let requirements = vec![requirement(json!({
                "kind":"action",
                "id":"response",
                "rule_key":"room",
                "action":action,
                "minimum":2
            }))];
            assert_eq!(
                normalize_turn_plan(&draft, &brief(), requirements)
                    .unwrap_err()
                    .code,
                "TURN_PLAN_INVALID_MINIMUM"
            );
        }

        let divergent_edit = vec![requirement(json!({
            "kind":"action",
            "id":"edit",
            "rule_key":"room",
            "action":{"kind":"edit_response","content":"New"},
            "minimum":1
        }))];
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), divergent_edit)
                .unwrap_err()
                .code,
            "TURN_PLAN_CONFLICT"
        );
    }

    #[test]
    fn existing_exact_actions_must_precede_missing_actions_in_the_same_rule() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[{"type":"create_role","key":"member_role","name":"Members"}]
            }]
        }))
        .unwrap();
        let existing = requirement(json!({
            "kind":"action",
            "id":"role",
            "rule_key":"room",
            "action":{"kind":"create_role","key":"member_role","name":"Members"},
            "minimum":1
        }));
        let missing = requirement(json!({
            "kind":"action",
            "id":"channel",
            "rule_key":"room",
            "action":{"kind":"create_channel","key":"room_channel","name":"Room"},
            "minimum":1
        }));

        let error = normalize_turn_plan(&draft, &brief(), vec![missing.clone(), existing.clone()])
            .unwrap_err();
        let accepted = normalize_turn_plan(&draft, &brief(), vec![existing, missing]).unwrap();

        assert_eq!(error.code, "TURN_PLAN_EXISTING_ACTION_ORDER");
        assert_eq!(accepted.len(), 3);
    }

    #[test]
    fn created_action_keys_share_one_rule_namespace_without_validation() {
        let mut empty_rule = Draft::new();
        empty_rule.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[]
            }]
        }))
        .unwrap();
        let planned_collision = vec![
            requirement(json!({
                "kind":"action",
                "id":"role",
                "rule_key":"room",
                "action":{"kind":"create_role","key":"shared","name":"Members"},
                "minimum":1
            })),
            requirement(json!({
                "kind":"action",
                "id":"channel",
                "rule_key":"room",
                "action":{"kind":"create_channel","key":"shared","name":"Room"},
                "minimum":1
            })),
        ];

        let mut existing_role = empty_rule.clone();
        existing_role.ruleset.rules[0].actions = vec![ActionSpec::CreateRole {
            key: "shared".to_string(),
            name: "Members".to_string(),
        }];
        let existing_collision = vec![requirement(json!({
            "kind":"action",
            "id":"register",
            "rule_key":"room",
            "action":{
                "kind":"register_instance",
                "key":"shared",
                "instance_kind":"room",
                "resources":{
                    "roles":[{"alias":"member","created":"shared"}],
                    "channels":[],
                    "messages":[]
                }
            },
            "minimum":1
        }))];

        assert!(!brief().verification.validate);
        assert_eq!(
            normalize_turn_plan(&empty_rule, &brief(), planned_collision)
                .unwrap_err()
                .code,
            "TURN_PLAN_CREATED_KEY_CONFLICT"
        );
        assert_eq!(
            normalize_turn_plan(&existing_role, &brief(), existing_collision)
                .unwrap_err()
                .code,
            "TURN_PLAN_CREATED_KEY_CONFLICT"
        );
    }

    #[test]
    fn plan_order_must_match_defer_register_and_edit_tool_placement() {
        let mut draft = Draft::new();
        draft.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[{"type":"create_channel","key":"room_channel","name":"Room"}]
            }]
        }))
        .unwrap();
        let response = requirement(json!({
            "kind":"action",
            "id":"response",
            "rule_key":"room",
            "action":{"kind":"respond_ephemeral","content":"Ready"},
            "minimum":1
        }));
        let defer = requirement(json!({
            "kind":"action",
            "id":"defer",
            "rule_key":"room",
            "action":{"kind":"defer_ephemeral"},
            "minimum":1
        }));
        let register = requirement(json!({
            "kind":"action",
            "id":"register",
            "rule_key":"room",
            "action":{
                "kind":"register_instance",
                "key":"room_instance",
                "instance_kind":"room",
                "resources":{
                    "roles":[],
                    "channels":[{"alias":"room","created":"room_channel"}],
                    "messages":[]
                }
            },
            "minimum":1
        }));
        let edit = requirement(json!({
            "kind":"action",
            "id":"edit",
            "rule_key":"room",
            "action":{"kind":"edit_response","content":"Created"},
            "minimum":1
        }));

        assert_eq!(
            normalize_turn_plan(&draft, &brief(), vec![response, defer.clone()])
                .unwrap_err()
                .code,
            "TURN_PLAN_ACTION_ORDER"
        );
        assert_eq!(
            normalize_turn_plan(&draft, &brief(), vec![edit.clone(), register.clone()])
                .unwrap_err()
                .code,
            "TURN_PLAN_ACTION_ORDER"
        );
        assert!(normalize_turn_plan(&draft, &brief(), vec![register, edit]).is_ok());
        assert!(normalize_turn_plan(
            &draft,
            &brief(),
            vec![
                defer,
                requirement(json!({
                    "kind":"action",
                    "id":"after_defer",
                    "rule_key":"room",
                    "action":{"kind":"respond_ephemeral","content":"Ready"},
                    "minimum":1
                }))
            ]
        )
        .is_err());
    }

    #[test]
    fn final_candidate_must_preserve_the_accepted_merged_action_order() {
        let mut root = Draft::new();
        root.ruleset = serde_json::from_value(json!({
            "version":1,
            "panels":[],
            "modals":[],
            "rules":[{
                "key":"room",
                "trigger":{"type":"instance_action","action":"join"},
                "actions":[{"type":"create_channel","key":"room_channel","name":"Room"}]
            }]
        }))
        .unwrap();
        let requirements = normalize_turn_plan(
            &root,
            &brief(),
            vec![
                requirement(json!({
                    "kind":"action",
                    "id":"register",
                    "rule_key":"room",
                    "action":{
                        "kind":"register_instance",
                        "key":"room_instance",
                        "instance_kind":"room",
                        "resources":{
                            "roles":[],
                            "channels":[{"alias":"room","created":"room_channel"}],
                            "messages":[]
                        }
                    },
                    "minimum":1
                })),
                requirement(json!({
                    "kind":"action",
                    "id":"edit",
                    "rule_key":"room",
                    "action":{"kind":"edit_response","content":"Created"},
                    "minimum":1
                })),
            ],
        )
        .unwrap();
        let correct_actions = json!([
            {"type":"create_channel","key":"room_channel","name":"Room"},
            {
                "type":"register_instance",
                "key":"room_instance",
                "kind":"room",
                "resources":{
                    "roles":{},
                    "channels":{"room":{"created":"room_channel"}},
                    "messages":{}
                }
            },
            {"type":"edit_response","content":"Created"}
        ]);
        let wrong_actions = json!([
            {"type":"create_channel","key":"room_channel","name":"Room"},
            {"type":"edit_response","content":"Created"},
            {
                "type":"register_instance",
                "key":"room_instance",
                "kind":"room",
                "resources":{
                    "roles":{},
                    "channels":{"room":{"created":"room_channel"}},
                    "messages":{}
                }
            }
        ]);
        let mut correct = root.clone();
        correct.ruleset.rules[0].actions = serde_json::from_value(correct_actions).unwrap();
        let mut wrong = root.clone();
        wrong.ruleset.rules[0].actions = serde_json::from_value(wrong_actions).unwrap();

        assert!(validate_final_planned_action_order(&root, &correct, &requirements).is_ok());
        assert_eq!(
            validate_final_planned_action_order(&root, &wrong, &requirements)
                .unwrap_err()
                .code,
            "PLAN_FINAL_ACTION_ORDER_MISMATCH"
        );
    }
}
