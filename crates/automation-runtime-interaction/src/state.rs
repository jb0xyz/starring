#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractionReceiptStateV1 {
    Claimed,
    Acknowledging,
    Deferred,
    Prepared,
    Executing,
    Completed,
    Failed,
    RecoveryRequired,
}

impl InteractionReceiptStateV1 {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::RecoveryRequired
        )
    }

    pub fn is_in_flight(self) -> bool {
        matches!(
            self,
            Self::Claimed | Self::Acknowledging | Self::Deferred | Self::Prepared | Self::Executing
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractionAcknowledgementStateV1 {
    Unacknowledged,
    Attempting,
    Deferred,
    Responded,
    ResponseRecoveryTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InteractionReceiptPhaseV1 {
    state: InteractionReceiptStateV1,
    acknowledgement: InteractionAcknowledgementStateV1,
    action_plan_bound: bool,
}

impl InteractionReceiptPhaseV1 {
    pub fn new(
        state: InteractionReceiptStateV1,
        acknowledgement: InteractionAcknowledgementStateV1,
        action_plan_bound: bool,
    ) -> Result<Self, InteractionReceiptPhaseErrorV1> {
        validate_phase(state, acknowledgement, action_plan_bound)?;
        Ok(Self {
            state,
            acknowledgement,
            action_plan_bound,
        })
    }

    pub fn claimed() -> Self {
        Self {
            state: InteractionReceiptStateV1::Claimed,
            acknowledgement: InteractionAcknowledgementStateV1::Unacknowledged,
            action_plan_bound: false,
        }
    }

    pub fn state(self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub fn acknowledgement(self) -> InteractionAcknowledgementStateV1 {
        self.acknowledgement
    }

    pub fn action_plan_bound(self) -> bool {
        self.action_plan_bound
    }

    pub fn transition_to(self, next: Self) -> Result<Self, InteractionReceiptPhaseErrorV1> {
        validate_interaction_receipt_transition_v1(self, next)?;
        Ok(next)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionReceiptPhaseErrorV1 {
    #[error("interaction receipt phase is invalid")]
    InvalidPhase,
    #[error("interaction receipt acknowledgement regressed")]
    AcknowledgementRegression,
    #[error("interaction receipt action plan cannot become unbound")]
    ActionPlanRegression,
    #[error("interaction receipt transition is invalid")]
    InvalidTransition,
    #[error("terminal interaction receipt cannot transition")]
    TerminalTransition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionReceiptClaimDispositionV1 {
    Acquired,
    CompletedDuplicate { phase: InteractionReceiptPhaseV1 },
    InFlightDuplicate { phase: InteractionReceiptPhaseV1 },
    TerminalDuplicate { phase: InteractionReceiptPhaseV1 },
    RecoveryRequired,
    CorruptSemanticIdentity,
}

impl InteractionReceiptClaimDispositionV1 {
    pub fn duplicate_for(phase: InteractionReceiptPhaseV1) -> InteractionReceiptClaimDispositionV1 {
        match phase.state() {
            InteractionReceiptStateV1::Completed => Self::CompletedDuplicate { phase },
            InteractionReceiptStateV1::Failed => Self::TerminalDuplicate { phase },
            InteractionReceiptStateV1::RecoveryRequired => Self::RecoveryRequired,
            InteractionReceiptStateV1::Claimed
            | InteractionReceiptStateV1::Acknowledging
            | InteractionReceiptStateV1::Deferred
            | InteractionReceiptStateV1::Prepared
            | InteractionReceiptStateV1::Executing => Self::InFlightDuplicate { phase },
        }
    }

    pub fn owns_execution(self) -> bool {
        matches!(self, Self::Acquired)
    }
}

pub fn validate_interaction_receipt_transition_v1(
    current: InteractionReceiptPhaseV1,
    next: InteractionReceiptPhaseV1,
) -> Result<(), InteractionReceiptPhaseErrorV1> {
    validate_phase(
        current.state,
        current.acknowledgement,
        current.action_plan_bound,
    )?;
    validate_phase(next.state, next.acknowledgement, next.action_plan_bound)?;
    if current == next {
        return Err(InteractionReceiptPhaseErrorV1::InvalidTransition);
    }
    if current.state.is_terminal() {
        return Err(InteractionReceiptPhaseErrorV1::TerminalTransition);
    }
    if current.action_plan_bound && !next.action_plan_bound {
        return Err(InteractionReceiptPhaseErrorV1::ActionPlanRegression);
    }
    if !acknowledgement_can_advance(current.acknowledgement, next.acknowledgement) {
        return Err(InteractionReceiptPhaseErrorV1::AcknowledgementRegression);
    }
    let valid = match current.state {
        InteractionReceiptStateV1::Claimed => matches!(
            next.state,
            InteractionReceiptStateV1::Acknowledging
                | InteractionReceiptStateV1::Prepared
                | InteractionReceiptStateV1::Failed
                | InteractionReceiptStateV1::RecoveryRequired
        ),
        InteractionReceiptStateV1::Acknowledging => matches!(
            next.state,
            InteractionReceiptStateV1::Deferred
                | InteractionReceiptStateV1::Prepared
                | InteractionReceiptStateV1::Completed
                | InteractionReceiptStateV1::Failed
                | InteractionReceiptStateV1::RecoveryRequired
        ),
        InteractionReceiptStateV1::Deferred => matches!(
            next.state,
            InteractionReceiptStateV1::Prepared
                | InteractionReceiptStateV1::Failed
                | InteractionReceiptStateV1::RecoveryRequired
        ),
        InteractionReceiptStateV1::Prepared => matches!(
            next.state,
            InteractionReceiptStateV1::Acknowledging
                | InteractionReceiptStateV1::Executing
                | InteractionReceiptStateV1::Completed
                | InteractionReceiptStateV1::Failed
                | InteractionReceiptStateV1::RecoveryRequired
        ),
        InteractionReceiptStateV1::Executing => matches!(
            next.state,
            InteractionReceiptStateV1::Executing
                | InteractionReceiptStateV1::Completed
                | InteractionReceiptStateV1::Failed
                | InteractionReceiptStateV1::RecoveryRequired
        ),
        InteractionReceiptStateV1::Completed
        | InteractionReceiptStateV1::Failed
        | InteractionReceiptStateV1::RecoveryRequired => false,
    };
    if !valid || !transition_shape_is_valid(current, next) {
        return Err(InteractionReceiptPhaseErrorV1::InvalidTransition);
    }
    Ok(())
}

fn validate_phase(
    state: InteractionReceiptStateV1,
    acknowledgement: InteractionAcknowledgementStateV1,
    action_plan_bound: bool,
) -> Result<(), InteractionReceiptPhaseErrorV1> {
    let valid = match state {
        InteractionReceiptStateV1::Claimed => {
            acknowledgement == InteractionAcknowledgementStateV1::Unacknowledged
                && !action_plan_bound
        }
        InteractionReceiptStateV1::Acknowledging => {
            acknowledgement == InteractionAcknowledgementStateV1::Attempting
        }
        InteractionReceiptStateV1::Deferred => {
            acknowledgement == InteractionAcknowledgementStateV1::Deferred && !action_plan_bound
        }
        InteractionReceiptStateV1::Prepared => {
            action_plan_bound
                && matches!(
                    acknowledgement,
                    InteractionAcknowledgementStateV1::Unacknowledged
                        | InteractionAcknowledgementStateV1::Deferred
                        | InteractionAcknowledgementStateV1::Responded
                )
        }
        InteractionReceiptStateV1::Executing => {
            action_plan_bound
                && matches!(
                    acknowledgement,
                    InteractionAcknowledgementStateV1::Unacknowledged
                        | InteractionAcknowledgementStateV1::Attempting
                        | InteractionAcknowledgementStateV1::Deferred
                        | InteractionAcknowledgementStateV1::Responded
                )
        }
        InteractionReceiptStateV1::Completed => {
            action_plan_bound
                && matches!(
                    acknowledgement,
                    InteractionAcknowledgementStateV1::Unacknowledged
                        | InteractionAcknowledgementStateV1::Deferred
                        | InteractionAcknowledgementStateV1::Responded
                )
        }
        InteractionReceiptStateV1::Failed => true,
        InteractionReceiptStateV1::RecoveryRequired => true,
    };
    if !valid {
        return Err(InteractionReceiptPhaseErrorV1::InvalidPhase);
    }
    Ok(())
}

fn transition_shape_is_valid(
    current: InteractionReceiptPhaseV1,
    next: InteractionReceiptPhaseV1,
) -> bool {
    match next.state {
        InteractionReceiptStateV1::Acknowledging => {
            next.acknowledgement == InteractionAcknowledgementStateV1::Attempting
                && next.action_plan_bound == current.action_plan_bound
                && current.acknowledgement == InteractionAcknowledgementStateV1::Unacknowledged
        }
        InteractionReceiptStateV1::Deferred => {
            current.state == InteractionReceiptStateV1::Acknowledging
                && !current.action_plan_bound
                && next.acknowledgement == InteractionAcknowledgementStateV1::Deferred
        }
        InteractionReceiptStateV1::Prepared => {
            next.action_plan_bound
                && (matches!(
                    current.state,
                    InteractionReceiptStateV1::Claimed | InteractionReceiptStateV1::Deferred
                ) || (current.state == InteractionReceiptStateV1::Acknowledging
                    && current.action_plan_bound))
        }
        InteractionReceiptStateV1::Executing => {
            next.action_plan_bound
                && ((current.state == InteractionReceiptStateV1::Prepared
                    && current.acknowledgement == next.acknowledgement)
                    || current.state == InteractionReceiptStateV1::Executing)
        }
        InteractionReceiptStateV1::Completed => {
            next.action_plan_bound
                && matches!(
                    next.acknowledgement,
                    InteractionAcknowledgementStateV1::Unacknowledged
                        | InteractionAcknowledgementStateV1::Deferred
                        | InteractionAcknowledgementStateV1::Responded
                )
                && (current.state == InteractionReceiptStateV1::Executing
                    || (current.state == InteractionReceiptStateV1::Acknowledging
                        && current.action_plan_bound)
                    || (current.state == InteractionReceiptStateV1::Prepared
                        && current.acknowledgement == next.acknowledgement))
        }
        InteractionReceiptStateV1::Failed => {
            next.action_plan_bound == current.action_plan_bound
                && (next.acknowledgement == current.acknowledgement
                    || next.acknowledgement
                        == InteractionAcknowledgementStateV1::ResponseRecoveryTerminal)
        }
        InteractionReceiptStateV1::RecoveryRequired => {
            next.action_plan_bound == current.action_plan_bound
                && (next.acknowledgement == current.acknowledgement
                    || next.acknowledgement
                        == InteractionAcknowledgementStateV1::ResponseRecoveryTerminal)
        }
        InteractionReceiptStateV1::Claimed => false,
    }
}

fn acknowledgement_can_advance(
    current: InteractionAcknowledgementStateV1,
    next: InteractionAcknowledgementStateV1,
) -> bool {
    current == next
        || matches!(
            (current, next),
            (
                InteractionAcknowledgementStateV1::Unacknowledged,
                InteractionAcknowledgementStateV1::Attempting
                    | InteractionAcknowledgementStateV1::ResponseRecoveryTerminal
            ) | (
                InteractionAcknowledgementStateV1::Attempting,
                InteractionAcknowledgementStateV1::Deferred
                    | InteractionAcknowledgementStateV1::Responded
                    | InteractionAcknowledgementStateV1::ResponseRecoveryTerminal
            ) | (
                InteractionAcknowledgementStateV1::Deferred,
                InteractionAcknowledgementStateV1::Responded
                    | InteractionAcknowledgementStateV1::ResponseRecoveryTerminal
            )
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(
        state: InteractionReceiptStateV1,
        acknowledgement: InteractionAcknowledgementStateV1,
        action_plan_bound: bool,
    ) -> InteractionReceiptPhaseV1 {
        InteractionReceiptPhaseV1::new(state, acknowledgement, action_plan_bound).unwrap()
    }

    #[test]
    fn instance_defer_first_path_records_every_durable_checkpoint() {
        let claimed = InteractionReceiptPhaseV1::claimed();
        let acknowledging = phase(
            InteractionReceiptStateV1::Acknowledging,
            InteractionAcknowledgementStateV1::Attempting,
            false,
        );
        let deferred = phase(
            InteractionReceiptStateV1::Deferred,
            InteractionAcknowledgementStateV1::Deferred,
            false,
        );
        let prepared = phase(
            InteractionReceiptStateV1::Prepared,
            InteractionAcknowledgementStateV1::Deferred,
            true,
        );
        let executing = phase(
            InteractionReceiptStateV1::Executing,
            InteractionAcknowledgementStateV1::Deferred,
            true,
        );
        let completed = phase(
            InteractionReceiptStateV1::Completed,
            InteractionAcknowledgementStateV1::Deferred,
            true,
        );

        assert_eq!(claimed.transition_to(acknowledging).unwrap(), acknowledging);
        assert_eq!(acknowledging.transition_to(deferred).unwrap(), deferred);
        assert_eq!(deferred.transition_to(prepared).unwrap(), prepared);
        assert_eq!(prepared.transition_to(executing).unwrap(), executing);
        assert_eq!(executing.transition_to(completed).unwrap(), completed);
    }

    #[test]
    fn modal_prepare_first_path_preserves_bound_plan_across_acknowledgement() {
        let claimed = InteractionReceiptPhaseV1::claimed();
        let prepared = phase(
            InteractionReceiptStateV1::Prepared,
            InteractionAcknowledgementStateV1::Unacknowledged,
            true,
        );
        let acknowledging = phase(
            InteractionReceiptStateV1::Acknowledging,
            InteractionAcknowledgementStateV1::Attempting,
            true,
        );
        let completed = phase(
            InteractionReceiptStateV1::Completed,
            InteractionAcknowledgementStateV1::Responded,
            true,
        );

        assert_eq!(claimed.transition_to(prepared).unwrap(), prepared);
        assert_eq!(
            prepared.transition_to(acknowledging).unwrap(),
            acknowledging
        );
        assert_eq!(acknowledging.transition_to(completed).unwrap(), completed);
    }

    #[test]
    fn executing_state_preserves_independent_acknowledgement_progress() {
        let prepared = phase(
            InteractionReceiptStateV1::Prepared,
            InteractionAcknowledgementStateV1::Unacknowledged,
            true,
        );
        let executing = phase(
            InteractionReceiptStateV1::Executing,
            InteractionAcknowledgementStateV1::Unacknowledged,
            true,
        );
        let acknowledging = phase(
            InteractionReceiptStateV1::Executing,
            InteractionAcknowledgementStateV1::Attempting,
            true,
        );
        let completed = phase(
            InteractionReceiptStateV1::Completed,
            InteractionAcknowledgementStateV1::Responded,
            true,
        );
        let recovery = phase(
            InteractionReceiptStateV1::RecoveryRequired,
            InteractionAcknowledgementStateV1::Unacknowledged,
            true,
        );
        let completed_without_response = phase(
            InteractionReceiptStateV1::Completed,
            InteractionAcknowledgementStateV1::Unacknowledged,
            true,
        );

        assert_eq!(prepared.transition_to(executing).unwrap(), executing);
        assert_eq!(
            executing.transition_to(acknowledging).unwrap(),
            acknowledging
        );
        assert_eq!(acknowledging.transition_to(completed).unwrap(), completed);
        assert_eq!(executing.transition_to(recovery).unwrap(), recovery);
        assert_eq!(
            executing.transition_to(completed_without_response).unwrap(),
            completed_without_response
        );
    }

    #[test]
    fn execution_requires_a_bound_plan_and_plan_binding_is_monotonic() {
        assert_eq!(
            InteractionReceiptPhaseV1::new(
                InteractionReceiptStateV1::Executing,
                InteractionAcknowledgementStateV1::Deferred,
                false,
            ),
            Err(InteractionReceiptPhaseErrorV1::InvalidPhase)
        );
        let prepared = phase(
            InteractionReceiptStateV1::Prepared,
            InteractionAcknowledgementStateV1::Deferred,
            true,
        );
        let failed_without_plan = phase(
            InteractionReceiptStateV1::Failed,
            InteractionAcknowledgementStateV1::Deferred,
            false,
        );
        assert_eq!(
            prepared.transition_to(failed_without_plan),
            Err(InteractionReceiptPhaseErrorV1::ActionPlanRegression)
        );
    }

    #[test]
    fn acknowledgement_attempt_cannot_regress_or_skip_durable_intent() {
        let claimed = InteractionReceiptPhaseV1::claimed();
        let responded = phase(
            InteractionReceiptStateV1::Failed,
            InteractionAcknowledgementStateV1::Responded,
            false,
        );
        assert_eq!(
            claimed.transition_to(responded),
            Err(InteractionReceiptPhaseErrorV1::AcknowledgementRegression)
        );
        let acknowledging = phase(
            InteractionReceiptStateV1::Acknowledging,
            InteractionAcknowledgementStateV1::Attempting,
            false,
        );
        let regressed = phase(
            InteractionReceiptStateV1::Failed,
            InteractionAcknowledgementStateV1::Unacknowledged,
            false,
        );
        assert_eq!(
            acknowledging.transition_to(regressed),
            Err(InteractionReceiptPhaseErrorV1::AcknowledgementRegression)
        );
    }

    #[test]
    fn response_recovery_terminal_preserves_definitive_and_ambiguous_outcomes() {
        let acknowledging = phase(
            InteractionReceiptStateV1::Acknowledging,
            InteractionAcknowledgementStateV1::Attempting,
            false,
        );
        let recovery = phase(
            InteractionReceiptStateV1::RecoveryRequired,
            InteractionAcknowledgementStateV1::ResponseRecoveryTerminal,
            false,
        );
        assert_eq!(acknowledging.transition_to(recovery).unwrap(), recovery);
        assert_eq!(
            InteractionReceiptPhaseV1::new(
                InteractionReceiptStateV1::Completed,
                InteractionAcknowledgementStateV1::ResponseRecoveryTerminal,
                true,
            ),
            Err(InteractionReceiptPhaseErrorV1::InvalidPhase)
        );
        let failed = phase(
            InteractionReceiptStateV1::Failed,
            InteractionAcknowledgementStateV1::ResponseRecoveryTerminal,
            false,
        );
        assert_eq!(acknowledging.transition_to(failed).unwrap(), failed);
    }

    #[test]
    fn terminal_states_cannot_transition_and_duplicates_never_own_execution() {
        let completed = phase(
            InteractionReceiptStateV1::Completed,
            InteractionAcknowledgementStateV1::Responded,
            true,
        );
        let recovery = phase(
            InteractionReceiptStateV1::RecoveryRequired,
            InteractionAcknowledgementStateV1::Responded,
            true,
        );
        let failed = phase(
            InteractionReceiptStateV1::Failed,
            InteractionAcknowledgementStateV1::Responded,
            true,
        );
        assert_eq!(
            completed.transition_to(recovery),
            Err(InteractionReceiptPhaseErrorV1::TerminalTransition)
        );
        let disposition = InteractionReceiptClaimDispositionV1::duplicate_for(completed);
        assert_eq!(
            disposition,
            InteractionReceiptClaimDispositionV1::CompletedDuplicate { phase: completed }
        );
        assert!(!disposition.owns_execution());
        assert!(InteractionReceiptClaimDispositionV1::Acquired.owns_execution());
        assert_eq!(
            InteractionReceiptClaimDispositionV1::duplicate_for(recovery),
            InteractionReceiptClaimDispositionV1::RecoveryRequired
        );
        assert_eq!(
            InteractionReceiptClaimDispositionV1::duplicate_for(failed),
            InteractionReceiptClaimDispositionV1::TerminalDuplicate { phase: failed }
        );
    }
}
