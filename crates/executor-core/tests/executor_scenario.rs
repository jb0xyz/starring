use std::collections::BTreeMap;

use approval_manager::ApprovalRequest;
use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use executor_core::{
    AdapterCall, AdapterError, AdapterErrorKind, ApprovedExecutionRequest, CreatedResource,
    Executor, ExecutorError, JobStatus, MockDiscordAdapter, RollbackAction, RollbackStatus,
    StepOutcome,
};
use futures::executor::block_on;
use operation_graph::compile_operations;
use policy_engine::Verdict;

fn before_guild() -> GuildState {
    GuildState {
        guild: Guild {
            id: GuildId(1),
            name: "srv".to_string(),
            owner_id: UserId(1),
        },
        roles: vec![Role {
            id: RoleId(1),
            name: "everyone".to_string(),
            permissions: Permissions::VIEW_CHANNEL,
            position: 0,
            managed: false,
        }],
        channels: vec![Channel {
            id: ChannelId(500),
            name: "general".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }],
        }],
        members: vec![],
    }
}

fn desired() -> DesiredState {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant {
            allow: vec![Capability::View, Capability::Send],
            deny: vec![],
        },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity {
                key: verified,
                ..Default::default()
            },
            name: Some("Verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("general".to_string()),
                ..Default::default()
            },
            name: Some("general".to_string()),
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

fn request(verdict: Verdict) -> ApprovedExecutionRequest {
    let before = before_guild();
    let normalized = compile(&desired()).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&before));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval: ApprovalRequest::new(verdict, UserId(1)),
        snapshot: before,
        guild_id: GuildId(1),
        requested_by: UserId(1),
        approved_by: vec![UserId(1)],
    }
}

#[test]
fn success_executes_all_ops_and_threads_created_id() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();

    assert_eq!(result.status, JobStatus::Succeeded);
    assert!(result
        .steps
        .iter()
        .all(|s| matches!(s.outcome, StepOutcome::Success)));

    let created_role = result
        .steps
        .iter()
        .find_map(|s| match &s.created {
            Some(CreatedResource::Role { id, .. }) => Some(id),
            _ => None,
        })
        .copied();
    assert_eq!(created_role, Some(RoleId(900_000)));

    let create_step = result
        .steps
        .iter()
        .find(|s| matches!(s.created, Some(CreatedResource::Role { .. })))
        .unwrap();
    assert_eq!(
        create_step.rollback,
        Some(RollbackAction::DeleteRole {
            id: RoleId(900_000)
        })
    );

    let calls = executor.adapter().calls();
    assert!(matches!(
        calls.first(),
        Some(AdapterCall::CreateRole { .. })
    ));
    assert!(calls.iter().any(|c| matches!(
        c,
        AdapterCall::UpsertOverwrite {
            target: OverwriteTarget::Role(RoleId(900_000)),
            ..
        }
    )));
}

#[test]
fn fail_fast_stops_and_skips_rest() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        2,
        AdapterError::new(AdapterErrorKind::MissingPermissions, "no perms"),
    ));
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();

    assert_eq!(result.status, JobStatus::Failed);
    assert!(matches!(result.steps[0].outcome, StepOutcome::Success));
    assert!(matches!(
        result.steps[1].outcome,
        StepOutcome::FailedFatal(_)
    ));
    assert!(result.steps[2..]
        .iter()
        .all(|s| matches!(s.outcome, StepOutcome::Skipped)));
    assert_eq!(executor.adapter().calls().len(), 2);
}

#[test]
fn retryable_failure_also_stops() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        1,
        AdapterError::new(AdapterErrorKind::RateLimited, "rl"),
    ));
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();
    assert_eq!(result.status, JobStatus::Failed);
    assert!(matches!(
        result.steps[0].outcome,
        StepOutcome::FailedRetryable(_)
    ));
}

#[test]
fn not_approved_refuses_without_calls() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let err = block_on(executor.execute(&request(Verdict::Deny))).unwrap_err();
    assert_eq!(err, ExecutorError::NotApproved);
    assert_eq!(executor.adapter().calls().len(), 0);
}

#[test]
fn job_result_serde_roundtrips() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let result = block_on(executor.execute(&request(Verdict::Allow))).unwrap();
    let json = serde_json::to_string(&result).unwrap();
    assert_eq!(
        serde_json::from_str::<executor_core::JobResult>(&json).unwrap(),
        result
    );
}

#[test]
fn success_needs_no_rollback() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let run = block_on(executor.execute_with_rollback(&request(Verdict::Allow))).unwrap();
    assert_eq!(run.job.status, JobStatus::Succeeded);
    assert_eq!(run.rollback.status, RollbackStatus::NotRequired);
    assert!(run.rollback.steps.is_empty());
}

#[test]
fn failure_triggers_rollback_of_created_role() {
    let executor = Executor::new(MockDiscordAdapter::with_failure(
        2,
        AdapterError::new(AdapterErrorKind::MissingPermissions, "no"),
    ));
    let run = block_on(executor.execute_with_rollback(&request(Verdict::Allow))).unwrap();
    assert_eq!(run.job.status, JobStatus::Failed);
    assert_eq!(run.rollback.status, RollbackStatus::Succeeded);
    assert_eq!(run.rollback.steps.len(), 1);
    assert!(matches!(
        run.rollback.steps[0].action,
        RollbackAction::DeleteRole { .. }
    ));
}

#[test]
fn not_approved_skips_rollback() {
    let executor = Executor::new(MockDiscordAdapter::new());
    let err = block_on(executor.execute_with_rollback(&request(Verdict::Deny))).unwrap_err();
    assert_eq!(err, ExecutorError::NotApproved);
    assert_eq!(executor.adapter().calls().len(), 0);
}
