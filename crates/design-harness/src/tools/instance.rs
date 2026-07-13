use std::collections::BTreeMap;

use automation_state::{
    ActionSpec, ButtonRoute, CreatedRef, InstanceKind, InstanceRef, InstanceResourceRefs,
};

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::actions::insert_before_edit;
use super::{
    find_rule_mut, ManifestEntryInput, SetRegisterInstanceInput, PENDING_INSTANCE_REFERENCE,
};

pub(super) fn set_register_instance(
    draft: &mut Draft,
    input: SetRegisterInstanceInput,
) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.rule_key)?;
    let existing_registration =
        rule.actions
            .iter()
            .enumerate()
            .find_map(|(index, action)| match action {
                ActionSpec::RegisterInstance { key, .. } => Some((index, key.clone())),
                _ => None,
            });

    let ownable = ownable_resources(rule);
    if ownable.is_empty() {
        return Err(StructuredError::new(
            "EMPTY_INSTANCE_RESOURCES",
            format!("rule.{}.actions", input.rule_key),
            "The instance-creation rule has no ownable resources",
            "Create a role, channel, or posted panel before set_register_instance",
        ));
    }
    let roles = manifest_map(&input.rule_key, "role", input.roles)?;
    let channels = manifest_map(&input.rule_key, "channel", input.channels)?;
    let messages = manifest_map(&input.rule_key, "message", input.messages)?;
    ensure_complete_manifest(&input.rule_key, &ownable, &roles, &channels, &messages)?;

    for action in &mut rule.actions {
        if let ActionSpec::PostPanel { buttons, .. } = action {
            for button in buttons {
                if let ButtonRoute::InstanceAction { instance, .. } = &mut button.route {
                    if matches!(
                        instance,
                        InstanceRef::Created(reference)
                            if reference.created == PENDING_INSTANCE_REFERENCE
                                || existing_registration
                                    .as_ref()
                                    .is_some_and(|(_, key)| reference.created == *key)
                    ) {
                        *instance = InstanceRef::Created(CreatedRef {
                            created: input.instance_key.clone(),
                        });
                    }
                }
            }
        }
    }

    let action = ActionSpec::RegisterInstance {
        key: input.instance_key.clone(),
        kind: InstanceKind(input.kind),
        resources: InstanceResourceRefs {
            roles,
            channels,
            messages,
        },
    };
    if let Some((index, _)) = existing_registration {
        rule.actions.remove(index);
    }
    insert_before_edit(rule, action);
    Ok(format!(
        "Finalized instance {} for rule {}",
        input.instance_key, input.rule_key
    ))
}

fn ownable_resources(rule: &automation_state::InteractionRule) -> BTreeMap<String, &'static str> {
    rule.actions
        .iter()
        .filter_map(|action| match action {
            ActionSpec::CreateRole { key, .. } => Some((key.clone(), "role")),
            ActionSpec::CreateChannel { key, .. } => Some((key.clone(), "channel")),
            ActionSpec::PostPanel { key, .. } => Some((key.clone(), "message")),
            _ => None,
        })
        .collect()
}

fn manifest_map(
    rule_key: &str,
    kind: &str,
    entries: Vec<ManifestEntryInput>,
) -> Result<BTreeMap<String, CreatedRef>, StructuredError> {
    let mut result = BTreeMap::new();
    for entry in entries {
        if result
            .insert(
                entry.alias.clone(),
                CreatedRef {
                    created: entry.created,
                },
            )
            .is_some()
        {
            return Err(StructuredError::new(
                "DUPLICATE_INSTANCE_RESOURCE_ALIAS",
                format!("rule.{rule_key}.register.{kind}.{}", entry.alias),
                "The instance manifest repeats an alias",
                "Use each manifest alias once",
            ));
        }
    }
    Ok(result)
}

fn ensure_complete_manifest(
    rule_key: &str,
    ownable: &BTreeMap<String, &'static str>,
    roles: &BTreeMap<String, CreatedRef>,
    channels: &BTreeMap<String, CreatedRef>,
    messages: &BTreeMap<String, CreatedRef>,
) -> Result<(), StructuredError> {
    let mut counts = BTreeMap::new();
    let mut declared = BTreeMap::new();
    for reference in roles.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "role");
    }
    for reference in channels.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "channel");
    }
    for reference in messages.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "message");
    }

    for (key, kind) in ownable {
        match declared.get(key) {
            None => {
                return Err(StructuredError::new(
                    "INSTANCE_RESOURCE_MISSING",
                    format!("rule.{rule_key}.actions"),
                    format!("Created {kind} {key} is missing from the instance manifest"),
                    format!("Add {key} once to the {kind} manifest"),
                ));
            }
            Some(declared_kind) if declared_kind != kind => {
                return Err(StructuredError::new(
                    "INSTANCE_RESOURCE_TYPE_MISMATCH",
                    format!("rule.{rule_key}.register"),
                    format!("Created resource {key} is declared as {declared_kind}"),
                    format!("Move {key} to the {kind} manifest"),
                ));
            }
            Some(_) => {}
        }
    }
    for (key, count) in counts {
        if count > 1 {
            return Err(StructuredError::new(
                "INSTANCE_RESOURCE_DECLARED_MULTIPLE_TIMES",
                format!("rule.{rule_key}.register"),
                format!("Created resource {key} is declared more than once"),
                format!("Keep exactly one manifest entry for {key}"),
            ));
        }
        if !ownable.contains_key(&key) {
            return Err(StructuredError::new(
                "UNRESOLVED_CREATED_REFERENCE",
                format!("rule.{rule_key}.register"),
                format!("Manifest resource {key} has not been created"),
                format!("Add the matching create action before registering {key}"),
            ));
        }
    }
    Ok(())
}
