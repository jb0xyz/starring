use std::collections::BTreeMap;
use std::env;

use approval_manager::ApprovalRequest;
use bot_runtime::TwilightDiscordAdapter;
use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use executor_core::{
    ApprovedExecutionRequest, DiscordAdapter, Executor, JobResult, RollbackAction, StepOutcome,
};
use operation_graph::compile_operations;
use policy_engine::Verdict;

fn fixture(guild_id: GuildId) -> ApprovedExecutionRequest {
    let verified = ResourceKey("starring-smoke-verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View],
            deny: vec![],
        },
    );
    let desired = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified,
                ..Default::default()
            },
            name: Some("starring-smoke-verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("starring-smoke-channel".to_string()),
                ..Default::default()
            },
            name: Some("starring-smoke-channel".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: None,
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    };
    let snapshot = GuildState {
        guild: Guild {
            id: guild_id,
            name: "smoke".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![],
        channels: vec![],
        members: vec![],
    };
    let normalized = compile(&desired).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&snapshot));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval: ApprovalRequest::new(Verdict::Allow, UserId(1)),
        snapshot,
        guild_id,
        requested_by: UserId(1),
        approved_by: vec![UserId(1)],
    }
}

async fn cleanup(adapter: &TwilightDiscordAdapter, guild: GuildId, result: &JobResult) {
    for step in result.steps.iter().rev() {
        if !matches!(step.outcome, StepOutcome::Success) {
            continue;
        }
        let Some(rollback) = &step.rollback else {
            continue;
        };
        let outcome = match rollback {
            RollbackAction::DeleteRole { id } => adapter.delete_role(guild, *id).await,
            RollbackAction::DeleteChannel { id } => adapter.delete_channel(guild, *id).await,
            other => {
                println!("  skip rollback (channel deletion covers overwrites): {other:?}");
                continue;
            }
        };
        match outcome {
            Ok(()) => println!("  rolled back: {rollback:?}"),
            Err(e) => println!("  rollback FAILED ({e:?}) - manual cleanup may be needed"),
        }
    }
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = match env::var("DISCORD_TEST_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("set DISCORD_TEST_TOKEN and DISCORD_TEST_GUILD to run the smoke");
            return;
        }
    };
    let guild_raw = match env::var("DISCORD_TEST_GUILD")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        Some(g) => g,
        None => {
            eprintln!("set DISCORD_TEST_GUILD to a numeric guild id");
            return;
        }
    };
    let guild_id = GuildId(guild_raw);

    let request = fixture(guild_id);
    let executor = Executor::new(TwilightDiscordAdapter::new(token));
    println!("executing smoke fixture against guild {guild_raw} ...");

    match executor.execute(&request).await {
        Ok(result) => {
            println!("job status: {:?}", result.status);
            for step in &result.steps {
                println!("  {:?}: {:?}", step.op_id, step.outcome);
            }
            println!("cleaning up (rollback, reverse order) ...");
            cleanup(executor.adapter(), guild_id, &result).await;
            println!("done. verify 'starring-smoke-*' resources are gone in the test guild.");
        }
        Err(e) => eprintln!("execution refused: {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_builds_three_creates_and_is_executable() {
        let request = fixture(GuildId(123));
        assert_eq!(request.operation_graph.nodes.len(), 3);
        assert!(request.approval.can_execute());
    }
}
