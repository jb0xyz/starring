use automation_core::{handle_event, AutomationServices};
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
    ruleset_key: &str,
    mutation: &TwilightMutationAdapter<'_>,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    interaction: &Interaction,
    failure_message: &str,
    instances: &impl InstanceStore,
    instance_ids: &impl InstanceIdGenerator,
) {
    let Some(event) = interaction_to_event(interaction, ruleset_key) else {
        return;
    };
    let responder = TwilightInteractionResponder::from_interaction(http, interaction, ruleset_key);
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
        ruleset_key,
    )
    .await
    {
        Ok(outcome) => eprintln!("interaction {} -> {outcome:?}", interaction.id.get()),
        Err(error) => eprintln!("interaction {} failed: {error:?}", interaction.id.get()),
    }
}
