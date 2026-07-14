use std::collections::{btree_map::Entry, BTreeMap};

use automation_instance::{InstanceId, InstanceIdGenerator, InstanceMessageRef, InstanceResources};
use automation_state::{ButtonRoute, ButtonSpec, InstanceRef, InstanceResourceRefs};
use discord_model::{ChannelId, RoleId};

use crate::adapter::{AdapterError, AdapterErrorKind, PostPanelButtonSpec, ResolvedButtonRoute};
use crate::event::RuntimeContext;
use crate::plan::{ActionPlan, PlannedAction, PlannedChannel, PlannedRole};

#[derive(Default)]
pub(super) struct ExecutionState {
    pub(super) created_roles: BTreeMap<String, RoleId>,
    pub(super) created_channels: BTreeMap<String, ChannelId>,
    pub(super) created_messages: BTreeMap<String, InstanceMessageRef>,
    pub(super) planned_instances: BTreeMap<String, InstanceId>,
    pub(super) created_instances: BTreeMap<String, InstanceId>,
}

impl ExecutionState {
    pub(super) fn prepare<G>(plan: &ActionPlan, instance_ids: &G) -> Result<Self, AdapterError>
    where
        G: InstanceIdGenerator,
    {
        let mut planned_instances = BTreeMap::new();
        for step in &plan.steps {
            if let PlannedAction::RegisterInstance { key, .. } = step {
                if let Entry::Vacant(entry) = planned_instances.entry(key.clone()) {
                    let id = instance_ids.generate().map_err(instance_id_error)?;
                    entry.insert(id);
                }
            }
        }
        Ok(Self {
            planned_instances,
            ..Self::default()
        })
    }

    pub(super) fn resolve_role(
        &self,
        role: &PlannedRole,
        context: &RuntimeContext,
    ) -> Result<RoleId, AdapterError> {
        match role {
            PlannedRole::Resolved(id) => Ok(*id),
            PlannedRole::Created(key) => self
                .created_roles
                .get(key)
                .copied()
                .ok_or_else(|| unresolved_created_role(key)),
            PlannedRole::Instance { alias } => {
                let resolved = context
                    .instance
                    .as_ref()
                    .ok_or_else(|| instance_context_missing(alias))?;
                resolved
                    .instance
                    .resources
                    .roles
                    .get(alias)
                    .copied()
                    .ok_or_else(|| instance_resource_not_found(&resolved.instance.id, alias))
            }
        }
    }

    pub(super) fn resolve_channel(
        &self,
        channel: &PlannedChannel,
    ) -> Result<ChannelId, AdapterError> {
        match channel {
            PlannedChannel::Resolved(id) => Ok(*id),
            PlannedChannel::Created(key) => self
                .created_channels
                .get(key)
                .copied()
                .ok_or_else(|| unresolved_created_channel(key)),
        }
    }

    pub(super) fn resolve_panel_buttons(
        &self,
        buttons: &[ButtonSpec],
        context: &RuntimeContext,
    ) -> Result<Vec<PostPanelButtonSpec>, AdapterError> {
        buttons
            .iter()
            .map(|button| {
                let route = match &button.route {
                    ButtonRoute::Static { key } => ResolvedButtonRoute::Static { key: key.clone() },
                    ButtonRoute::InstanceAction { instance, action } => {
                        ResolvedButtonRoute::InstanceAction {
                            instance_id: self.resolve_button_instance(instance, context)?,
                            action: action.clone(),
                        }
                    }
                };
                Ok(PostPanelButtonSpec {
                    label: button.label.clone(),
                    route,
                })
            })
            .collect()
    }

    pub(super) fn resolve_instance_ref(
        &self,
        instance: &InstanceRef,
        context: &RuntimeContext,
    ) -> Result<InstanceId, AdapterError> {
        match instance {
            InstanceRef::Event => context
                .instance
                .as_ref()
                .map(|resolved| resolved.instance.id.clone())
                .ok_or_else(event_instance_missing),
            InstanceRef::Created(created) => self
                .created_instances
                .get(&created.created)
                .or_else(|| self.planned_instances.get(&created.created))
                .cloned()
                .ok_or_else(|| unresolved_planned_instance(&created.created)),
        }
    }

    pub(super) fn resolve_manifest(
        &self,
        refs: &InstanceResourceRefs,
    ) -> Result<InstanceResources, AdapterError> {
        let mut resources = InstanceResources::default();
        for (alias, created) in &refs.roles {
            let id = self
                .created_roles
                .get(&created.created)
                .copied()
                .ok_or_else(|| unresolved_manifest(&created.created))?;
            resources.roles.insert(alias.clone(), id);
        }
        for (alias, created) in &refs.channels {
            let id = self
                .created_channels
                .get(&created.created)
                .copied()
                .ok_or_else(|| unresolved_manifest(&created.created))?;
            resources.channels.insert(alias.clone(), id);
        }
        for (alias, created) in &refs.messages {
            let message = self
                .created_messages
                .get(&created.created)
                .cloned()
                .ok_or_else(|| unresolved_manifest(&created.created))?;
            resources.messages.insert(alias.clone(), message);
        }
        Ok(resources)
    }

    fn resolve_button_instance(
        &self,
        instance: &InstanceRef,
        context: &RuntimeContext,
    ) -> Result<InstanceId, AdapterError> {
        match instance {
            InstanceRef::Created(created) => self
                .planned_instances
                .get(&created.created)
                .cloned()
                .ok_or_else(|| unresolved_planned_instance(&created.created)),
            InstanceRef::Event => context
                .instance
                .as_ref()
                .map(|resolved| resolved.instance.id.clone())
                .ok_or_else(event_instance_missing),
        }
    }
}

pub(super) fn unresolved_planned_instance(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved planned instance: {key}"),
    )
}

fn unresolved_manifest(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved manifest ref: {key}"),
    )
}

fn instance_id_error(error: automation_instance::InstanceIdGenerationError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("instance id error: {error:?}"),
    )
}

fn unresolved_created_role(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created role: {key}"),
    )
}

fn unresolved_created_channel(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created channel: {key}"),
    )
}

fn event_instance_missing() -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        "InstanceContextMissing: button route",
    )
}

fn instance_context_missing(alias: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("InstanceContextMissing: role alias={alias}"),
    )
}

fn instance_resource_not_found(instance_id: &InstanceId, alias: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("InstanceResourceNotFound: instance={instance_id}, resource=role, alias={alias}"),
    )
}
