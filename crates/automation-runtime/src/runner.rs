use automation_core::{
    handle_event, AutomationServices, EventKind, HandleOutcome, RunningRuleSetIdentity,
};
use automation_instance::{InstanceIdGenerator, InstanceStore};
use automation_instance_teardown::InstanceTeardownService;
use automation_ruleset::RuleSetStore;
use automation_ruleset_dispatch::{
    dispatch_instance_action, DispatchFailure, GuildRoleSnapshotProvider,
};
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;

use crate::convert::interaction_to_event;
use crate::mutation::TwilightMutationAdapter;
use crate::responder::TwilightInteractionResponder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionExecutionCategoryV3 {
    Ignored,
    Static,
    Instance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionExecutionOutcomeV3 {
    Ignored,
    StaticExecuted,
    StaticNoOp,
    StaticFailed,
    InstanceExecuted,
    InstanceNoOp,
    InstanceFailed,
}

impl InteractionExecutionOutcomeV3 {
    pub fn category(self) -> InteractionExecutionCategoryV3 {
        match self {
            Self::Ignored => InteractionExecutionCategoryV3::Ignored,
            Self::StaticExecuted | Self::StaticNoOp | Self::StaticFailed => {
                InteractionExecutionCategoryV3::Static
            }
            Self::InstanceExecuted | Self::InstanceNoOp | Self::InstanceFailed => {
                InteractionExecutionCategoryV3::Instance
            }
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Ignored => "interaction_ignored",
            Self::StaticExecuted => "interaction_static_executed",
            Self::StaticNoOp => "interaction_static_no_op",
            Self::StaticFailed => "interaction_static_failed",
            Self::InstanceExecuted => "interaction_instance_executed",
            Self::InstanceNoOp => "interaction_instance_no_op",
            Self::InstanceFailed => "interaction_instance_failed",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_interaction(
    http: &Client,
    identity: &RunningRuleSetIdentity,
    mutation: &TwilightMutationAdapter<'_>,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    interaction: &Interaction,
    failure_message: &str,
    instances: &impl InstanceStore,
    instance_ids: &impl InstanceIdGenerator,
    teardown: &impl InstanceTeardownService,
    ruleset_store: &impl RuleSetStore,
    snapshot_provider: &impl GuildRoleSnapshotProvider,
) -> InteractionExecutionOutcomeV3 {
    let Some(event) = interaction_to_event(interaction, &identity.key) else {
        return InteractionExecutionOutcomeV3::Ignored;
    };
    let responder =
        TwilightInteractionResponder::from_interaction(http, interaction, &identity.key);
    let services = AutomationServices {
        mutation,
        responder: &responder,
        instances,
        instance_ids,
        teardown,
    };
    match &event.kind {
        EventKind::ButtonClick { .. } | EventKind::ModalSubmit { .. } => static_outcome(
            handle_event(
                &event,
                ruleset,
                bindings,
                &services,
                failure_message,
                identity,
            )
            .await,
        ),
        EventKind::InstanceAction {
            instance_id,
            action,
        } => instance_outcome(
            dispatch_instance_action(
                &event,
                instance_id,
                action,
                ruleset_store,
                snapshot_provider,
                bindings,
                &services,
                failure_message,
            )
            .await,
        ),
    }
}

fn static_outcome(
    result: Result<HandleOutcome, automation_core::AdapterError>,
) -> InteractionExecutionOutcomeV3 {
    match result {
        Ok(HandleOutcome::Executed) => InteractionExecutionOutcomeV3::StaticExecuted,
        Ok(HandleOutcome::NoOp) => InteractionExecutionOutcomeV3::StaticNoOp,
        Err(_) => InteractionExecutionOutcomeV3::StaticFailed,
    }
}

fn instance_outcome(
    result: Result<HandleOutcome, DispatchFailure>,
) -> InteractionExecutionOutcomeV3 {
    match result {
        Ok(HandleOutcome::Executed) => InteractionExecutionOutcomeV3::InstanceExecuted,
        Ok(HandleOutcome::NoOp) => InteractionExecutionOutcomeV3::InstanceNoOp,
        Err(_) => InteractionExecutionOutcomeV3::InstanceFailed,
    }
}

#[cfg(test)]
mod tests {
    use automation_core::{AdapterError, AdapterErrorKind};
    use automation_instance::InstanceStoreError;
    use automation_ruleset_dispatch::{DispatchError, DispatchFailure, FailureResponseOutcome};

    use super::*;

    #[test]
    fn ignored_outcome_is_stable_and_redacted() {
        let outcome = InteractionExecutionOutcomeV3::Ignored;
        assert_eq!(outcome.category(), InteractionExecutionCategoryV3::Ignored);
        assert_eq!(outcome.code(), "interaction_ignored");
        assert_eq!(format!("{outcome:?}"), "Ignored");
    }

    #[test]
    fn static_failure_does_not_retain_backend_message() {
        let secret = "static backend secret";
        let outcome = static_outcome(Err(AdapterError::new(AdapterErrorKind::Unknown, secret)));
        let details = format!("{outcome:?} {}", outcome.code());
        assert_eq!(outcome, InteractionExecutionOutcomeV3::StaticFailed);
        assert_eq!(outcome.category(), InteractionExecutionCategoryV3::Static);
        assert!(!details.contains(secret));
    }

    #[test]
    fn instance_failure_does_not_retain_backend_message() {
        let secret = "instance backend secret";
        let outcome = instance_outcome(Err(DispatchFailure {
            cause: DispatchError::InstanceLookup(InstanceStoreError::Backend(secret.to_string())),
            failure_response: FailureResponseOutcome::Failed(AdapterError::new(
                AdapterErrorKind::Unknown,
                secret,
            )),
        }));
        let details = format!("{outcome:?} {}", outcome.code());
        assert_eq!(outcome, InteractionExecutionOutcomeV3::InstanceFailed);
        assert_eq!(outcome.category(), InteractionExecutionCategoryV3::Instance);
        assert!(!details.contains(secret));
    }
}
