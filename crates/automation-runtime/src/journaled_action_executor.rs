mod effect_call;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Debug, Formatter};

use automation_core::preflight::{ActionEntryIdV1, PreflightedActionPlanV1, PreparedPlanActionV1};
use automation_core::{InteractionResponder, RunResult};
use automation_instance::InstanceRegistrarV1;
use automation_instance_teardown::{
    DurableInstanceTeardownServiceV1, ExactInstanceTeardownRequestV1,
};
use automation_runtime_interaction::{
    InteractionEffectActionIndexV1, InteractionEffectAttemptOutcomeV1,
    InteractionEffectIndeterminateClassV1, InteractionEffectRecoveryScopeV1,
};

use crate::discord_effects::RecoverableDiscordMutationAdapterV1;
use crate::effect_journal::{
    InteractionEffectIntentDispositionV1, InteractionEffectJournalIntendV1,
    InteractionEffectJournalPlanV1, InteractionEffectJournalPortV1,
    InteractionEffectPlanBindDispositionV1,
};
use crate::interaction_effect_plan::{
    InteractionEffectExecutionPlanEntryV1, InteractionEffectExecutionPlanV1,
};

use self::effect_call::{
    execute_effect_call_v1, materialize_effect_v1, record_success_v1,
    JournaledActionExecutionStateV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournaledActionExecutionStageV1 {
    Projection,
    PlanBind,
    Materialization,
    EffectIntent,
    EffectCall,
    EffectFinish,
    InitialResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournaledActionExecutionStopReasonV1 {
    ProtocolViolation,
    JournalUnavailable,
    ExactReplaySuppressed,
    KnownFailure,
    Indeterminate,
    ResponseFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournaledActionExecutionStopV1 {
    stage: JournaledActionExecutionStageV1,
    reason: JournaledActionExecutionStopReasonV1,
    action_entry: Option<ActionEntryIdV1>,
    effect_index: Option<InteractionEffectActionIndexV1>,
    recovery_scope: Option<InteractionEffectRecoveryScopeV1>,
    rollback_requested: bool,
}

impl JournaledActionExecutionStopV1 {
    pub fn stage(self) -> JournaledActionExecutionStageV1 {
        self.stage
    }

    pub fn reason(self) -> JournaledActionExecutionStopReasonV1 {
        self.reason
    }

    pub fn action_entry(self) -> Option<ActionEntryIdV1> {
        self.action_entry
    }

    pub fn effect_index(self) -> Option<InteractionEffectActionIndexV1> {
        self.effect_index
    }

    pub fn recovery_scope(self) -> Option<InteractionEffectRecoveryScopeV1> {
        self.recovery_scope
    }

    pub fn rollback_requested(self) -> bool {
        self.rollback_requested
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournaledActionExecutionOutcomeV1 {
    Completed(RunResult),
    Stopped {
        stop: JournaledActionExecutionStopV1,
        durable_result: RunResult,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExactInteractionTeardownSetErrorV1 {
    #[error("an exact interaction teardown action entry is duplicated")]
    DuplicateEntry,
}

#[derive(Clone, Default)]
pub struct ExactInteractionTeardownSetV1 {
    requests: BTreeMap<ActionEntryIdV1, ExactInstanceTeardownRequestV1>,
}

impl ExactInteractionTeardownSetV1 {
    pub fn new(
        requests: impl IntoIterator<Item = (ActionEntryIdV1, ExactInstanceTeardownRequestV1)>,
    ) -> Result<Self, ExactInteractionTeardownSetErrorV1> {
        let mut exact = BTreeMap::new();
        for (entry, request) in requests {
            if exact.insert(entry, request).is_some() {
                return Err(ExactInteractionTeardownSetErrorV1::DuplicateEntry);
            }
        }
        Ok(Self { requests: exact })
    }

    fn get(&self, entry: ActionEntryIdV1) -> Option<&ExactInstanceTeardownRequestV1> {
        self.requests.get(&entry)
    }
}

impl Debug for ExactInteractionTeardownSetV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExactInteractionTeardownSetV1")
            .field("request_count", &self.requests.len())
            .finish()
    }
}

pub struct JournaledActionExecutionServicesV1<'a, J, M, R, S, T> {
    pub journal: &'a J,
    pub mutation: &'a M,
    pub responder: &'a R,
    pub instances: &'a S,
    pub teardown: &'a T,
    pub exact_teardowns: &'a ExactInteractionTeardownSetV1,
}

pub(crate) async fn execute_journaled_action_plan_v1<J, M, R, S, T>(
    plan: &PreflightedActionPlanV1,
    effect_plan: &InteractionEffectExecutionPlanV1,
    journal_plan: &InteractionEffectJournalPlanV1,
    leading_defer_ephemeral: bool,
    services: &JournaledActionExecutionServicesV1<'_, J, M, R, S, T>,
) -> JournaledActionExecutionOutcomeV1
where
    J: InteractionEffectJournalPortV1,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    T: DurableInstanceTeardownServiceV1,
{
    let projection =
        match validate_projection_v1(plan, effect_plan, journal_plan, leading_defer_ephemeral) {
            Ok(projection) => projection,
            Err(()) => {
                return stopped_v1(
                    JournaledActionExecutionStageV1::Projection,
                    JournaledActionExecutionStopReasonV1::ProtocolViolation,
                    None,
                    None,
                    None,
                    false,
                    RunResult::default(),
                );
            }
        };
    let mut state = JournaledActionExecutionStateV1::default();
    match services.journal.bind_effect_plan_v1(journal_plan).await {
        Ok(InteractionEffectPlanBindDispositionV1::Fresh) => {}
        Ok(InteractionEffectPlanBindDispositionV1::ExactReplay) => {
            return stopped_v1(
                JournaledActionExecutionStageV1::PlanBind,
                JournaledActionExecutionStopReasonV1::ExactReplaySuppressed,
                None,
                None,
                None,
                false,
                state.result,
            );
        }
        Err(_) => {
            return stopped_v1(
                JournaledActionExecutionStageV1::PlanBind,
                JournaledActionExecutionStopReasonV1::JournalUnavailable,
                None,
                None,
                None,
                false,
                state.result,
            );
        }
    }
    for entry in &projection.mutable_effects {
        if let Some(outcome) = execute_one_effect_v1(plan, entry, services, &mut state).await {
            return outcome;
        }
    }
    for action in &projection.initial_responses {
        let (entry, result) = match action {
            PreparedPlanActionV1::RespondEphemeral { entry, content } => (
                *entry,
                services.responder.respond_ephemeral(content.clone()).await,
            ),
            PreparedPlanActionV1::OpenModal { entry, modal } => {
                (*entry, services.responder.open_modal(modal).await)
            }
            _ => unreachable!(),
        };
        if result.is_err() {
            return stopped_v1(
                JournaledActionExecutionStageV1::InitialResponse,
                JournaledActionExecutionStopReasonV1::ResponseFailure,
                Some(entry),
                None,
                Some(InteractionEffectRecoveryScopeV1::ResponseTail),
                false,
                state.result,
            );
        }
    }
    if let Some(entry) = projection.response_tail {
        if let Some(outcome) = execute_one_effect_v1(plan, entry, services, &mut state).await {
            return outcome;
        }
    }
    JournaledActionExecutionOutcomeV1::Completed(state.result)
}

async fn execute_one_effect_v1<J, M, R, S, T>(
    plan: &PreflightedActionPlanV1,
    entry: &InteractionEffectExecutionPlanEntryV1,
    services: &JournaledActionExecutionServicesV1<'_, J, M, R, S, T>,
    state: &mut JournaledActionExecutionStateV1,
) -> Option<JournaledActionExecutionOutcomeV1>
where
    J: InteractionEffectJournalPortV1,
    M: RecoverableDiscordMutationAdapterV1,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    T: DurableInstanceTeardownServiceV1,
{
    let definition = entry.definition();
    let effect_index = definition.action().action_index();
    let scope = definition.recovery_scope();
    let materialized = match materialize_effect_v1(definition, state) {
        Ok(materialized) => materialized,
        Err(()) => {
            return Some(stopped_v1(
                JournaledActionExecutionStageV1::Materialization,
                JournaledActionExecutionStopReasonV1::ProtocolViolation,
                Some(entry.action_entry()),
                Some(effect_index),
                Some(scope),
                rollback_for_prior_v1(scope, state),
                std::mem::take(&mut state.result),
            ));
        }
    };
    let prepared = match effect_call::prepare_effect_call_v1(
        entry,
        &materialized,
        state,
        services.exact_teardowns,
        plan.context(),
    ) {
        Ok(prepared) => prepared,
        Err(()) => {
            return Some(stopped_v1(
                JournaledActionExecutionStageV1::Materialization,
                JournaledActionExecutionStopReasonV1::ProtocolViolation,
                Some(entry.action_entry()),
                Some(effect_index),
                Some(scope),
                rollback_for_prior_v1(scope, state),
                std::mem::take(&mut state.result),
            ));
        }
    };
    let intent = InteractionEffectJournalIntendV1::new(
        &materialized,
        prepared.resolved_instance_manifest_digest_v1(),
    );
    let permit = match services.journal.intend_effect_v1(intent).await {
        Ok(InteractionEffectIntentDispositionV1::ExternalCallAuthorized(permit)) => permit,
        Ok(InteractionEffectIntentDispositionV1::ExactReplay) => {
            return Some(stopped_v1(
                JournaledActionExecutionStageV1::EffectIntent,
                JournaledActionExecutionStopReasonV1::ExactReplaySuppressed,
                Some(entry.action_entry()),
                Some(effect_index),
                Some(scope),
                rollback_for_prior_v1(scope, state),
                std::mem::take(&mut state.result),
            ));
        }
        Err(_) => {
            return Some(stopped_v1(
                JournaledActionExecutionStageV1::EffectIntent,
                JournaledActionExecutionStopReasonV1::JournalUnavailable,
                Some(entry.action_entry()),
                Some(effect_index),
                Some(scope),
                rollback_for_prior_v1(scope, state),
                std::mem::take(&mut state.result),
            ));
        }
    };
    let call = execute_effect_call_v1(prepared, &materialized, services).await;
    if services
        .journal
        .finish_effect_v1(&permit, &materialized, &call.outcome)
        .await
        .is_err()
    {
        return Some(stopped_v1(
            JournaledActionExecutionStageV1::EffectFinish,
            JournaledActionExecutionStopReasonV1::JournalUnavailable,
            Some(entry.action_entry()),
            Some(effect_index),
            Some(scope),
            scope == InteractionEffectRecoveryScopeV1::MutableProvisioning,
            std::mem::take(&mut state.result),
        ));
    }
    match &call.outcome {
        InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) => {
            if record_success_v1(
                entry,
                output,
                call.teardown,
                call.registered_instance,
                state,
            )
            .is_err()
            {
                return Some(stopped_v1(
                    JournaledActionExecutionStageV1::EffectCall,
                    JournaledActionExecutionStopReasonV1::ProtocolViolation,
                    Some(entry.action_entry()),
                    Some(effect_index),
                    Some(scope),
                    scope == InteractionEffectRecoveryScopeV1::MutableProvisioning,
                    std::mem::take(&mut state.result),
                ));
            }
            None
        }
        InteractionEffectAttemptOutcomeV1::KnownFailed(_) => Some(stopped_v1(
            JournaledActionExecutionStageV1::EffectCall,
            JournaledActionExecutionStopReasonV1::KnownFailure,
            Some(entry.action_entry()),
            Some(effect_index),
            Some(scope),
            rollback_for_prior_v1(scope, state),
            std::mem::take(&mut state.result),
        )),
        InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::DeadlineElapsed
            | InteractionEffectIndeterminateClassV1::ConnectionLost
            | InteractionEffectIndeterminateClassV1::Cancelled
            | InteractionEffectIndeterminateClassV1::MalformedResponse
            | InteractionEffectIndeterminateClassV1::PersistenceCommit
            | InteractionEffectIndeterminateClassV1::ProviderUnavailable
            | InteractionEffectIndeterminateClassV1::Unknown,
        ) => Some(stopped_v1(
            JournaledActionExecutionStageV1::EffectCall,
            JournaledActionExecutionStopReasonV1::Indeterminate,
            Some(entry.action_entry()),
            Some(effect_index),
            Some(scope),
            scope == InteractionEffectRecoveryScopeV1::MutableProvisioning,
            std::mem::take(&mut state.result),
        )),
    }
}

struct ProjectionV1<'a> {
    mutable_effects: Vec<&'a InteractionEffectExecutionPlanEntryV1>,
    initial_responses: Vec<&'a PreparedPlanActionV1>,
    response_tail: Option<&'a InteractionEffectExecutionPlanEntryV1>,
}

fn validate_projection_v1<'a>(
    plan: &'a PreflightedActionPlanV1,
    effect_plan: &'a InteractionEffectExecutionPlanV1,
    journal_plan: &InteractionEffectJournalPlanV1,
    leading_defer_ephemeral: bool,
) -> Result<ProjectionV1<'a>, ()> {
    if effect_plan.snapshot_digest() != journal_plan.snapshot_digest()
        || effect_plan.entries().len() != journal_plan.entries().len()
        || effect_plan
            .entries()
            .iter()
            .zip(journal_plan.entries())
            .any(|(execution, journal)| {
                execution.definition() != journal.definition()
                    || execution.expected_postimage_digest() != journal.expected_postimage_digest()
                    || execution
                        .definition()
                        .action()
                        .preflight_certificate_digest()
                        != journal_plan.preflight_certificate_digest()
            })
    {
        return Err(());
    }
    let original = plan
        .actions()
        .iter()
        .map(|action| (action.entry(), action))
        .collect::<BTreeMap<_, _>>();
    if original.len() != plan.actions().len() {
        return Err(());
    }
    let mut seen = BTreeSet::new();
    let mut mutable_effects = Vec::new();
    let mut response_tail = None;
    for entry in effect_plan.entries() {
        if original.get(&entry.action_entry()).copied() != Some(entry.action())
            || !seen.insert(entry.action_entry())
        {
            return Err(());
        }
        if matches!(entry.action(), PreparedPlanActionV1::EditResponse { .. }) {
            if response_tail.replace(entry).is_some() {
                return Err(());
            }
        } else if is_mutable_effect_v1(entry.action()) {
            if response_tail.is_some() {
                return Err(());
            }
            mutable_effects.push(entry);
        } else {
            return Err(());
        }
    }
    let initial_responses = plan
        .actions()
        .iter()
        .filter(|action| {
            matches!(
                action,
                PreparedPlanActionV1::RespondEphemeral { .. }
                    | PreparedPlanActionV1::OpenModal { .. }
            )
        })
        .collect::<Vec<_>>();
    if initial_responses.len() > 1 || !initial_responses.is_empty() && response_tail.is_some() {
        return Err(());
    }
    if !effect_plan.entries().is_empty() {
        let final_response_tail = response_tail.is_some_and(|tail| {
            plan.actions()
                .last()
                .is_some_and(|action| action.entry() == tail.action_entry())
        });
        if !initial_responses.is_empty() || !leading_defer_ephemeral || !final_response_tail {
            return Err(());
        }
    }
    let last_mutable = mutable_effects
        .last()
        .map(|entry| entry.action_entry().ordinal());
    if initial_responses
        .iter()
        .any(|response| last_mutable.is_some_and(|last| response.entry().ordinal() <= last))
    {
        return Err(());
    }
    let expected_effect_entries = plan
        .actions()
        .iter()
        .filter(|action| {
            is_mutable_effect_v1(action)
                || matches!(action, PreparedPlanActionV1::EditResponse { .. })
        })
        .map(PreparedPlanActionV1::entry)
        .collect::<BTreeSet<_>>();
    if expected_effect_entries != seen {
        return Err(());
    }
    Ok(ProjectionV1 {
        mutable_effects,
        initial_responses,
        response_tail,
    })
}

fn is_mutable_effect_v1(action: &PreparedPlanActionV1) -> bool {
    matches!(
        action,
        PreparedPlanActionV1::GrantRole { .. }
            | PreparedPlanActionV1::CreateChannel { .. }
            | PreparedPlanActionV1::CreateRole { .. }
            | PreparedPlanActionV1::UpsertOverwrite { .. }
            | PreparedPlanActionV1::PostPanel { .. }
            | PreparedPlanActionV1::RegisterInstance { .. }
            | PreparedPlanActionV1::TeardownInstance { .. }
    )
}

fn rollback_for_prior_v1(
    scope: InteractionEffectRecoveryScopeV1,
    state: &JournaledActionExecutionStateV1,
) -> bool {
    scope == InteractionEffectRecoveryScopeV1::MutableProvisioning && state.mutable_successes > 0
}

#[allow(clippy::too_many_arguments)]
fn stopped_v1(
    stage: JournaledActionExecutionStageV1,
    reason: JournaledActionExecutionStopReasonV1,
    action_entry: Option<ActionEntryIdV1>,
    effect_index: Option<InteractionEffectActionIndexV1>,
    recovery_scope: Option<InteractionEffectRecoveryScopeV1>,
    rollback_requested: bool,
    durable_result: RunResult,
) -> JournaledActionExecutionOutcomeV1 {
    JournaledActionExecutionOutcomeV1::Stopped {
        stop: JournaledActionExecutionStopV1 {
            stage,
            reason,
            action_entry,
            effect_index,
            recovery_scope,
            rollback_requested,
        },
        durable_result,
    }
}
