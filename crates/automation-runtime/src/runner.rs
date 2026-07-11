use automation_core::{handle_event, AutomationServices, RunningRuleSetIdentity};
use automation_instance::{InstanceIdGenerator, InstanceStore};
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;

use crate::convert::interaction_to_event;
use crate::mutation::TwilightMutationAdapter;
use crate::responder::TwilightInteractionResponder;

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
) {
    let Some(event) = interaction_to_event(interaction, &identity.key) else {
        return;
    };
    let responder =
        TwilightInteractionResponder::from_interaction(http, interaction, &identity.key);
    let services = AutomationServices {
        mutation,
        responder: &responder,
        instances,
        instance_ids,
    };
    match handle_event(
        &event,
        ruleset,
        bindings,
        &services,
        failure_message,
        identity,
    )
    .await
    {
        Ok(outcome) => eprintln!("interaction {} -> {outcome:?}", interaction.id.get()),
        Err(error) => eprintln!("interaction {} failed: {error:?}", interaction.id.get()),
    }
}
