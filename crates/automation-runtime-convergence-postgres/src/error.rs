use automation_runtime_convergence::RuntimeDeploymentError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeConvergenceStoreError {
    #[error("runtime deployment was not found")]
    NotFound,
    #[error("runtime deployment scope does not match")]
    ScopeMismatch,
    #[error("runtime deployment idempotency identity conflicts")]
    IdempotencyConflict,
    #[error("runtime deployment revision conflicts")]
    RevisionConflict,
    #[error("runtime deployment target is not the exact active pointer")]
    ActiveTargetMismatch,
    #[error("runtime deployment binding authority changed")]
    BindingAuthorityMismatch,
    #[error("runtime deployment product authority is inactive")]
    ProductAuthorityInactive,
    #[error("runtime serving lease is owned by another fenced process")]
    ServingLeaseConflict,
    #[error("runtime attestation identity conflicts")]
    AttestationConflict,
    #[error("runtime convergence attempt conflicts")]
    ConvergenceAttemptConflict,
    #[error("runtime convergence attempt capacity is exhausted")]
    ConvergenceAttemptOverflow,
    #[error("runtime convergence retry is not ready")]
    RetryNotReady,
    #[error("runtime deployment requires an operator action")]
    OperatorActionRequired,
    #[error("runtime convergence input is invalid: {0}")]
    InvalidInput(&'static str),
    #[error("persisted runtime convergence state is invalid: {0}")]
    InvalidPersistedState(&'static str),
    #[error(transparent)]
    Domain(#[from] RuntimeDeploymentError),
    #[error("runtime convergence database operation timed out")]
    DatabaseTimeout,
    #[error("runtime convergence database transaction must be retried")]
    DatabaseConcurrency,
    #[error("runtime convergence database is unavailable")]
    DatabaseUnavailable,
    #[error("runtime convergence database operation failed")]
    DatabaseFailure,
}

impl RuntimeConvergenceStoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "runtime_not_found",
            Self::ScopeMismatch => "runtime_scope_mismatch",
            Self::IdempotencyConflict => "runtime_idempotency_conflict",
            Self::RevisionConflict => "runtime_revision_conflict",
            Self::ActiveTargetMismatch => "runtime_active_target_mismatch",
            Self::BindingAuthorityMismatch => "runtime_binding_authority_mismatch",
            Self::ProductAuthorityInactive => "runtime_product_authority_inactive",
            Self::ServingLeaseConflict => "runtime_serving_lease_conflict",
            Self::AttestationConflict => "runtime_attestation_conflict",
            Self::ConvergenceAttemptConflict => "runtime_convergence_attempt_conflict",
            Self::ConvergenceAttemptOverflow => "runtime_convergence_attempt_overflow",
            Self::RetryNotReady => "runtime_retry_not_ready",
            Self::OperatorActionRequired => "runtime_operator_action_required",
            Self::InvalidInput(_) => "runtime_invalid_input",
            Self::InvalidPersistedState(_) => "runtime_invalid_persisted_state",
            Self::Domain(_) => "runtime_domain_rejected",
            Self::DatabaseTimeout => "runtime_database_timeout",
            Self::DatabaseConcurrency => "runtime_database_concurrency",
            Self::DatabaseUnavailable => "runtime_database_unavailable",
            Self::DatabaseFailure => "runtime_database_failure",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RetryNotReady
                | Self::DatabaseTimeout
                | Self::DatabaseConcurrency
                | Self::DatabaseUnavailable
        )
    }
}

pub(crate) fn database(error: sqlx::Error) -> RuntimeConvergenceStoreError {
    let state = error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|state| state.into_owned());
    match state.as_deref() {
        Some("55P03" | "57014") => RuntimeConvergenceStoreError::DatabaseTimeout,
        Some("40001" | "40P01") => RuntimeConvergenceStoreError::DatabaseConcurrency,
        Some(state) if state.starts_with("08") => RuntimeConvergenceStoreError::DatabaseUnavailable,
        Some("53300" | "57P01" | "57P02" | "57P03") => {
            RuntimeConvergenceStoreError::DatabaseUnavailable
        }
        _ if matches!(
            error,
            sqlx::Error::Io(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::PoolTimedOut
                | sqlx::Error::PoolClosed
                | sqlx::Error::WorkerCrashed
        ) =>
        {
            RuntimeConvergenceStoreError::DatabaseUnavailable
        }
        _ => RuntimeConvergenceStoreError::DatabaseFailure,
    }
}
