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
use discord_model::{ChannelType, GuildId, GuildState, Permissions, UserId};
use executor_core::{
    ApprovedExecutionRequest, DiscordAdapter, Executor, GuildStateReader, JobResult,
    RollbackAction, StepOutcome,
};
use operation_graph::compile_operations;
use policy_engine::{PolicyDecision, PolicyEngine};

fn simple() -> DesiredState {
    let verified = ResourceKey("smoke-verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View],
            deny: vec![],
        },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified.clone(),
                ..Default::default()
            },
            name: Some("starring-smoke-verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("smoke-channel".to_string()),
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
    }
}

fn game_community() -> DesiredState {
    let verified = ResourceKey("smoke-verified".to_string());
    let view_send = || {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant {
                allow: vec![Capability::View, Capability::Send],
                deny: vec![],
            },
        );
        AccessIntent {
            everyone: Some(AccessGrant {
                allow: vec![],
                deny: vec![Capability::View],
            }),
            roles,
        }
    };
    let view_only = || {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant {
                allow: vec![Capability::View],
                deny: vec![],
            },
        );
        AccessIntent {
            everyone: Some(AccessGrant {
                allow: vec![],
                deny: vec![Capability::View],
            }),
            roles,
        }
    };
    let channel = |key: &str, name: &str, access: Option<AccessIntent>| ChannelIntent {
        identity: Identity {
            key: ResourceKey(key.to_string()),
            ..Default::default()
        },
        name: Some(name.to_string()),
        channel_type: Some(ChannelType::Text),
        parent: None,
        access,
        raw_overwrites: None,
    };
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified.clone(),
                ..Default::default()
            },
            name: Some("starring-smoke-인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![
            channel("smoke-rules", "starring-smoke-규칙", None),
            channel("smoke-verify", "starring-smoke-인증", None),
            channel("smoke-general", "starring-smoke-일반", Some(view_send())),
            channel("smoke-questions", "starring-smoke-질문", Some(view_send())),
            channel("smoke-party", "starring-smoke-파티모집", Some(view_send())),
            channel("smoke-notice", "starring-smoke-공지", Some(view_only())),
        ],
        ..Default::default()
    }
}

fn no_admin() -> DesiredState {
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: ResourceKey("smoke-admin".to_string()),
                ..Default::default()
            },
            name: Some("starring-smoke-admin".to_string()),
            permissions: Some(Permissions::ADMINISTRATOR),
        }],
        ..Default::default()
    }
}

fn adopted() -> DesiredState {
    let role = ResourceKey("smoke-adopted".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        role.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: role,
                ..Default::default()
            },
            name: Some("starring-smoke-adopted".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("smoke-adopted-channel".to_string()),
                ..Default::default()
            },
            name: Some("starring-smoke-adopted-channel".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant {
                    allow: vec![],
                    deny: vec![Capability::View],
                }),
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

fn scenario(name: &str) -> DesiredState {
    match name {
        "game-community" => game_community(),
        "no-admin" => no_admin(),
        "adopted" => adopted(),
        _ => simple(),
    }
}

fn print_guild_state(state: &GuildState) {
    println!(
        "guild {}: {} roles, {} channels",
        state.guild.id.0,
        state.roles.len(),
        state.channels.len()
    );
    for role in &state.roles {
        println!("  role {} '{}'", role.id.0, role.name);
    }
    for channel in &state.channels {
        println!(
            "  channel {} '{}' {:?} ({} overwrites)",
            channel.id.0,
            channel.name,
            channel.channel_type,
            channel.overwrites.len()
        );
        for overwrite in &channel.overwrites {
            println!(
                "    {:?} allow={} deny={}",
                overwrite.target,
                overwrite.allow.bits(),
                overwrite.deny.bits()
            );
        }
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

    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "simple".to_string());

    let token = match env::var("DISCORD_TEST_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("set DISCORD_TEST_TOKEN and DISCORD_TEST_GUILD");
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
    let adapter = TwilightDiscordAdapter::new(token);

    if mode == "read" {
        match adapter.read_guild_state(guild_id).await {
            Ok(state) => print_guild_state(&state),
            Err(e) => eprintln!("read failed: {e:?}"),
        }
        return;
    }

    let desired = scenario(&mode);
    let before = match adapter.read_guild_state(guild_id).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("read before-state failed: {e:?}");
            return;
        }
    };
    println!(
        "before-state: {} roles, {} channels",
        before.roles.len(),
        before.channels.len()
    );

    let normalized = match compile(&desired) {
        Ok(n) => n,
        Err(errors) => {
            eprintln!("compile failed: {errors:?}");
            return;
        }
    };
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = match compile_operations(&diff_result, &normalized) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("operation graph failed: {e:?}");
            return;
        }
    };

    let decision: PolicyDecision = PolicyEngine::with_default_rules().evaluate(&graph);
    println!("scenario '{mode}': {} operations", graph.nodes.len());
    println!("policy verdict: {:?}", decision.verdict);
    for finding in &decision.findings {
        println!(
            "  finding [{}] {} - {}",
            finding.rule_id, finding.target, finding.message
        );
    }

    let mut approval = ApprovalRequest::new(decision.verdict, UserId(1));
    for approver in [UserId(10), UserId(11)] {
        if approval.can_execute() {
            break;
        }
        let _ = approval.approve(approver);
    }
    if !approval.can_execute() {
        println!(
            "NOT executing - approval state {:?}. no Discord change made.",
            approval.state()
        );
        return;
    }

    let request = ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval,
        snapshot: before,
        guild_id,
        requested_by: UserId(1),
        approved_by: vec![UserId(10), UserId(11)],
    };

    let executor = Executor::new(adapter);
    println!("executing '{mode}' against guild {guild_raw} ...");

    match executor.execute(&request).await {
        Ok(result) => {
            println!("job status: {:?}", result.status);
            for step in &result.steps {
                println!("  {:?}: {:?}", step.op_id, step.outcome);
            }
            match executor.adapter().read_guild_state(guild_id).await {
                Ok(after) => {
                    println!("after-state (read back from Discord):");
                    print_guild_state(&after);
                }
                Err(e) => eprintln!("read after-state failed: {e:?}"),
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
    fn simple_builds_two_creates() {
        let desired = simple();
        assert_eq!(desired.roles.len(), 1);
        assert_eq!(desired.channels.len(), 1);
    }

    #[test]
    fn game_community_has_role_and_six_channels() {
        let desired = game_community();
        assert_eq!(desired.roles.len(), 1);
        assert_eq!(desired.channels.len(), 6);
    }

    #[test]
    fn no_admin_grants_administrator() {
        let desired = no_admin();
        assert_eq!(
            desired.roles[0].permissions,
            Some(Permissions::ADMINISTRATOR)
        );
    }

    #[test]
    fn adopted_has_role_and_channel() {
        let desired = adopted();
        assert_eq!(desired.roles.len(), 1);
        assert_eq!(desired.channels.len(), 1);
    }
}
