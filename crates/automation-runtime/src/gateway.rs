use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;

use crate::mutation::TwilightMutationAdapter;
use crate::runner::handle_interaction;

pub async fn run(
    token: String,
    ruleset_key: String,
    ruleset: InteractionRuleSet,
    bindings: ResourceBindingMap,
) {
    let http = Client::new(token.clone());
    let mutation = TwilightMutationAdapter::new(&http, ruleset_key.clone());
    let mut shard = Shard::new(ShardId::ONE, token, Intents::empty());

    while let Some(item) = shard.next_event(EventTypeFlags::INTERACTION_CREATE).await {
        let event = match item {
            Ok(event) => event,
            Err(source) => {
                eprintln!("gateway receive error: {source}");
                continue;
            }
        };
        if let Event::InteractionCreate(interaction_create) = event {
            handle_interaction(
                &http,
                &ruleset_key,
                &mutation,
                &ruleset,
                &bindings,
                &interaction_create.0,
            )
            .await;
        }
    }
}
