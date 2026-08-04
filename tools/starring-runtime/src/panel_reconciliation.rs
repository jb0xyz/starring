use std::fmt::{Debug, Formatter};
use std::time::Duration;

use automation_panel_installation::strict::{StrictPanelReconcileRequestV1, StrictPanelReportV1};
use automation_panel_installation::InstallerError;
use automation_runtime::{
    render_strict_declared_panel_v1, OwnedDiscordRuntimeOperationsV2, PANEL_RENDER_REVISION,
};
use automation_runtime_controller::{
    RuntimeDeploymentScopeV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
};
use automation_runtime_convergence::{RuntimeDeploymentPhaseV1, RuntimeProcessIdentityV1};
use automation_runtime_convergence_postgres::{RuntimeConvergenceStoreError, RuntimeExactTargetV1};
use automation_runtime_panel_evidence::{CertifiedPanelEvidenceErrorV1, CertifiedPanelEvidenceV1};
use automation_runtime_panel_postgres::{
    PostgresRuntimePanelReconciliationV1, RuntimePanelPersistenceErrorV1,
    RuntimePanelReconciliationErrorV1, RuntimePanelReconciliationOutcomeV1,
    MAX_RUNTIME_PANEL_LEASE_HEADROOM,
};
use chrono::{DateTime, TimeDelta, Utc};
use tokio::time::{timeout_at, Instant as TokioInstant};

use crate::database::RuntimeControllerDatabaseV2;

const STRICT_PANEL_SIDE_EFFECT_HEADROOM: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub(crate) struct RuntimeExactPanelReconciliationV2 {
    database: RuntimeControllerDatabaseV2,
    discord: OwnedDiscordRuntimeOperationsV2,
}

impl RuntimeExactPanelReconciliationV2 {
    pub(crate) fn new(
        database: RuntimeControllerDatabaseV2,
        discord: OwnedDiscordRuntimeOperationsV2,
    ) -> Self {
        Self { database, discord }
    }

    pub(crate) async fn reconcile_exact_v2(
        &self,
        request: RuntimeExactPanelReconciliationRequestV2,
    ) -> Result<CertifiedPanelEvidenceV1, RuntimeExactPanelReconciliationErrorV2> {
        let now = Utc::now();
        validate_request_v2(&request, now)?;
        let deadline = runtime_deadline_v2(request.deadline, now)?;
        timeout_at(deadline, self.reconcile_inner_v2(request))
            .await
            .map_err(|_| RuntimeExactPanelReconciliationErrorV2::DeadlineElapsed)?
    }

    async fn reconcile_inner_v2(
        &self,
        request: RuntimeExactPanelReconciliationRequestV2,
    ) -> Result<CertifiedPanelEvidenceV1, RuntimeExactPanelReconciliationErrorV2> {
        let hydrated = self
            .database
            .exact_target()
            .load_for_execution(&request.execution)
            .await
            .map_err(RuntimeExactPanelReconciliationErrorV2::Hydration)?;
        validate_hydrated_target_v2(&request, &hydrated)?;
        let rendered = hydrated
            .artifact
            .definition
            .panels
            .iter()
            .enumerate()
            .map(|(index, panel)| {
                render_strict_declared_panel_v1(
                    hydrated.artifact.guild_id,
                    hydrated.artifact.ruleset_key.as_str(),
                    panel,
                )
                .map_err(|source| RuntimeExactPanelReconciliationErrorV2::Render { index, source })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target = hydrated.snapshot.target.clone();
        let ruleset_key = hydrated.artifact.ruleset_key.clone();
        let ruleset_version = hydrated.artifact.version;
        let bindings = hydrated.bindings.clone();
        let guard = execution_guard_v2(&request.execution);
        let store = self
            .database
            .panel()
            .claim(guard, hydrated, STRICT_PANEL_SIDE_EFFECT_HEADROOM)
            .await
            .map_err(RuntimeExactPanelReconciliationErrorV2::Claim)?;
        let installer = self.discord.strict_panel_installer();
        let outcome = PostgresRuntimePanelReconciliationV1::new(store)
            .run(
                StrictPanelReconcileRequestV1 {
                    guild_id: target.guild_id,
                    ruleset_key: &ruleset_key,
                    ruleset_version,
                    render_revision: PANEL_RENDER_REVISION,
                    panels: &rendered,
                    bindings: &bindings,
                },
                &installer,
            )
            .await
            .map_err(RuntimeExactPanelReconciliationErrorV2::Reconciliation)?;
        let report = match outcome {
            RuntimePanelReconciliationOutcomeV1::Eligible(report) => report,
            RuntimePanelReconciliationOutcomeV1::Ineligible(report) => {
                return Err(RuntimeExactPanelReconciliationErrorV2::Ineligible {
                    reason: RuntimePanelIneligibilityV2::from_report(&report),
                });
            }
        };
        let reconciled_at = Utc::now();
        if reconciled_at >= request.deadline {
            return Err(RuntimeExactPanelReconciliationErrorV2::DeadlineElapsed);
        }
        CertifiedPanelEvidenceV1::build(
            target,
            request.successor.runtime_generation,
            request.successor.process_instance_id,
            report,
            reconciled_at,
        )
        .map_err(RuntimeExactPanelReconciliationErrorV2::Certificate)
    }
}

impl Debug for RuntimeExactPanelReconciliationV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactPanelReconciliationV2(<redacted>)")
    }
}

pub(crate) struct RuntimeExactPanelReconciliationRequestV2 {
    execution: RuntimeExecutionReceiptV1,
    successor: RuntimeProcessIdentityV1,
    deadline: DateTime<Utc>,
}

impl RuntimeExactPanelReconciliationRequestV2 {
    pub(crate) fn new(
        execution: RuntimeExecutionReceiptV1,
        successor: RuntimeProcessIdentityV1,
        deadline: DateTime<Utc>,
    ) -> Self {
        Self {
            execution,
            successor,
            deadline,
        }
    }
}

impl Debug for RuntimeExactPanelReconciliationRequestV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeExactPanelReconciliationRequestV2(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RuntimeExactPanelReconciliationErrorV2 {
    #[error("runtime exact panel reconciliation request identity does not match")]
    RequestIdentityMismatch,
    #[error("runtime exact panel target identity changed")]
    HydratedIdentityMismatch,
    #[error("runtime exact panel reconciliation deadline is invalid")]
    InvalidDeadline,
    #[error("runtime exact panel reconciliation deadline elapsed")]
    DeadlineElapsed,
    #[error("runtime exact panel target hydration failed")]
    Hydration(#[source] RuntimeConvergenceStoreError),
    #[error("runtime strict panel rendering failed at index {index}")]
    Render {
        index: usize,
        #[source]
        source: InstallerError,
    },
    #[error("runtime strict panel authority claim failed")]
    Claim(#[source] RuntimePanelPersistenceErrorV1),
    #[error("runtime strict panel reconciliation failed")]
    Reconciliation(#[source] RuntimePanelReconciliationErrorV1),
    #[error("runtime strict panel reconciliation report is ineligible")]
    Ineligible { reason: RuntimePanelIneligibilityV2 },
    #[error("runtime strict panel certificate construction failed")]
    Certificate(#[source] CertifiedPanelEvidenceErrorV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeExactPanelReconciliationDispositionV2 {
    RetryableInfrastructure,
    ObservationRequired,
    DeploymentBlocked,
    AuthorityDrift,
    ProcessInvariant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimePanelIneligibilityV2 {
    Transient,
    Ambiguous,
    CleanupPending,
    UnresolvedChannel,
    Failed,
    Incomplete,
}

impl RuntimePanelIneligibilityV2 {
    fn from_report(report: &StrictPanelReportV1) -> Self {
        if report.ambiguous_outcome_count > 0 {
            Self::Ambiguous
        } else if report.stale_message_cleanup_pending_count > 0
            || report.orphan_message_cleanup_pending_count > 0
            || report.reposted_old_message_cleanup_pending_count > 0
        {
            Self::CleanupPending
        } else if report.skipped_transient_count > 0 {
            Self::Transient
        } else if report.skipped_unresolved_channel_count > 0 {
            Self::UnresolvedChannel
        } else if report.failed_count > 0 {
            Self::Failed
        } else {
            Self::Incomplete
        }
    }

    const fn disposition_v2(self) -> RuntimeExactPanelReconciliationDispositionV2 {
        match self {
            Self::Transient => {
                RuntimeExactPanelReconciliationDispositionV2::RetryableInfrastructure
            }
            Self::Ambiguous | Self::CleanupPending => {
                RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
            }
            Self::UnresolvedChannel | Self::Failed | Self::Incomplete => {
                RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
            }
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::Transient => "runtime_panel_transient",
            Self::Ambiguous => "runtime_panel_ambiguous",
            Self::CleanupPending => "runtime_panel_cleanup_pending",
            Self::UnresolvedChannel => "runtime_panel_channel_unresolved",
            Self::Failed => "runtime_panel_failed",
            Self::Incomplete => "runtime_panel_incomplete",
        }
    }
}

impl RuntimeExactPanelReconciliationErrorV2 {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::RequestIdentityMismatch => "runtime_panel_request_identity_mismatch",
            Self::HydratedIdentityMismatch => "runtime_panel_target_identity_changed",
            Self::InvalidDeadline => "runtime_panel_invalid_deadline",
            Self::DeadlineElapsed => "runtime_panel_deadline_elapsed",
            Self::Hydration(error) => error.code(),
            Self::Render { .. } => "runtime_panel_render_failed",
            Self::Claim(error) => runtime_panel_persistence_code_v2(error),
            Self::Reconciliation(RuntimePanelReconciliationErrorV1::Persistence(error)) => {
                runtime_panel_persistence_code_v2(error)
            }
            Self::Reconciliation(RuntimePanelReconciliationErrorV1::Strict(_)) => {
                "runtime_panel_reconciliation_invalid"
            }
            Self::Ineligible { reason } => reason.code(),
            Self::Certificate(_) => "runtime_panel_certificate_failed",
        }
    }

    pub(crate) fn disposition_v2(&self) -> RuntimeExactPanelReconciliationDispositionV2 {
        match self {
            Self::RequestIdentityMismatch | Self::InvalidDeadline => {
                RuntimeExactPanelReconciliationDispositionV2::ProcessInvariant
            }
            Self::HydratedIdentityMismatch => {
                RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift
            }
            Self::DeadlineElapsed => {
                RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
            }
            Self::Hydration(error) => runtime_hydration_disposition_v2(error),
            Self::Render { .. } => RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked,
            Self::Claim(error) => runtime_panel_persistence_disposition_v2(error),
            Self::Reconciliation(RuntimePanelReconciliationErrorV1::Persistence(error)) => {
                runtime_panel_persistence_disposition_v2(error)
            }
            Self::Reconciliation(RuntimePanelReconciliationErrorV1::Strict(_)) => {
                RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
            }
            Self::Ineligible { reason } => reason.disposition_v2(),
            Self::Certificate(_) => RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked,
        }
    }
}

fn runtime_hydration_disposition_v2(
    error: &RuntimeConvergenceStoreError,
) -> RuntimeExactPanelReconciliationDispositionV2 {
    match error {
        RuntimeConvergenceStoreError::RetryNotReady
        | RuntimeConvergenceStoreError::DatabaseTimeout
        | RuntimeConvergenceStoreError::DatabaseConcurrency
        | RuntimeConvergenceStoreError::DatabaseUnavailable => {
            RuntimeExactPanelReconciliationDispositionV2::RetryableInfrastructure
        }
        RuntimeConvergenceStoreError::ActiveTargetMismatch
        | RuntimeConvergenceStoreError::BindingAuthorityMismatch
        | RuntimeConvergenceStoreError::ProductAuthorityInactive
        | RuntimeConvergenceStoreError::RevisionConflict
        | RuntimeConvergenceStoreError::ConvergenceAttemptConflict
        | RuntimeConvergenceStoreError::ExecutionClaimStale
        | RuntimeConvergenceStoreError::ServingLeaseConflict => {
            RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift
        }
        RuntimeConvergenceStoreError::InvalidInput(_)
        | RuntimeConvergenceStoreError::DatabaseFailure
        | RuntimeConvergenceStoreError::DatabaseAuthorityMismatch => {
            RuntimeExactPanelReconciliationDispositionV2::ProcessInvariant
        }
        RuntimeConvergenceStoreError::NotFound
        | RuntimeConvergenceStoreError::ScopeMismatch
        | RuntimeConvergenceStoreError::IdempotencyConflict
        | RuntimeConvergenceStoreError::AttestationConflict
        | RuntimeConvergenceStoreError::ConvergenceAttemptOverflow
        | RuntimeConvergenceStoreError::OperatorActionRequired
        | RuntimeConvergenceStoreError::InvalidPersistedState(_)
        | RuntimeConvergenceStoreError::Domain(_) => {
            RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
        }
        _ => RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked,
    }
}

fn runtime_panel_persistence_disposition_v2(
    error: &RuntimePanelPersistenceErrorV1,
) -> RuntimeExactPanelReconciliationDispositionV2 {
    match error {
        RuntimePanelPersistenceErrorV1::RandomnessUnavailable
        | RuntimePanelPersistenceErrorV1::Conflict
        | RuntimePanelPersistenceErrorV1::Timeout
        | RuntimePanelPersistenceErrorV1::Unavailable => {
            RuntimeExactPanelReconciliationDispositionV2::RetryableInfrastructure
        }
        RuntimePanelPersistenceErrorV1::Indeterminate => {
            RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
        }
        RuntimePanelPersistenceErrorV1::OwnershipLost
        | RuntimePanelPersistenceErrorV1::AuthorityChanged => {
            RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift
        }
        RuntimePanelPersistenceErrorV1::Capacity
        | RuntimePanelPersistenceErrorV1::PersistenceCorrupt => {
            RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
        }
        RuntimePanelPersistenceErrorV1::InvalidAuthority => {
            RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift
        }
        RuntimePanelPersistenceErrorV1::InvalidDuration => {
            RuntimeExactPanelReconciliationDispositionV2::ProcessInvariant
        }
    }
}

fn runtime_panel_persistence_code_v2(error: &RuntimePanelPersistenceErrorV1) -> &'static str {
    match error {
        RuntimePanelPersistenceErrorV1::InvalidAuthority => "runtime_panel_invalid_authority",
        RuntimePanelPersistenceErrorV1::InvalidDuration => "runtime_panel_invalid_duration",
        RuntimePanelPersistenceErrorV1::RandomnessUnavailable => {
            "runtime_panel_randomness_unavailable"
        }
        RuntimePanelPersistenceErrorV1::OwnershipLost => "runtime_panel_ownership_lost",
        RuntimePanelPersistenceErrorV1::AuthorityChanged => "runtime_panel_authority_changed",
        RuntimePanelPersistenceErrorV1::Conflict => "runtime_panel_conflict",
        RuntimePanelPersistenceErrorV1::Capacity => "runtime_panel_capacity",
        RuntimePanelPersistenceErrorV1::PersistenceCorrupt => "runtime_panel_persistence_corrupt",
        RuntimePanelPersistenceErrorV1::Timeout => "runtime_panel_timeout",
        RuntimePanelPersistenceErrorV1::Unavailable => "runtime_panel_unavailable",
        RuntimePanelPersistenceErrorV1::Indeterminate => "runtime_panel_indeterminate",
    }
}

fn validate_request_v2(
    request: &RuntimeExactPanelReconciliationRequestV2,
    now: DateTime<Utc>,
) -> Result<(), RuntimeExactPanelReconciliationErrorV2> {
    let execution = &request.execution;
    let snapshot = &execution.snapshot;
    let Some(lease) = &snapshot.controller_lease else {
        return Err(RuntimeExactPanelReconciliationErrorV2::RequestIdentityMismatch);
    };
    if request.successor.target != snapshot.target
        || request.successor.runtime_generation != snapshot.runtime_generation
        || execution.controller_id != lease.controller_id
        || execution.fencing_token != lease.fencing_token
        || execution.acquired_at != lease.acquired_at
        || execution.expires_at != lease.expires_at
        || execution.acquired_at >= execution.expires_at
        || !matches!(snapshot.phase, RuntimeDeploymentPhaseV1::ReconcilingPanels)
    {
        return Err(RuntimeExactPanelReconciliationErrorV2::RequestIdentityMismatch);
    }
    validate_operation_window_v2(now, request.deadline, execution.expires_at)
}

fn validate_operation_window_v2(
    now: DateTime<Utc>,
    deadline: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
) -> Result<(), RuntimeExactPanelReconciliationErrorV2> {
    if STRICT_PANEL_SIDE_EFFECT_HEADROOM.is_zero()
        || STRICT_PANEL_SIDE_EFFECT_HEADROOM > MAX_RUNTIME_PANEL_LEASE_HEADROOM
    {
        return Err(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline);
    }
    let headroom = TimeDelta::from_std(STRICT_PANEL_SIDE_EFFECT_HEADROOM)
        .map_err(|_| RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)?;
    let latest_deadline = lease_expires_at
        .checked_sub_signed(headroom)
        .ok_or(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)?;
    if deadline <= now || deadline > latest_deadline {
        return Err(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline);
    }
    Ok(())
}

fn validate_hydrated_target_v2(
    request: &RuntimeExactPanelReconciliationRequestV2,
    hydrated: &RuntimeExactTargetV1,
) -> Result<(), RuntimeExactPanelReconciliationErrorV2> {
    let target = &request.execution.snapshot.target;
    let artifact = &hydrated.artifact;
    if hydrated.snapshot != request.execution.snapshot
        || artifact.guild_id != target.guild_id
        || artifact.ruleset_key != target.ruleset_key
        || artifact.version != target.version
        || artifact.content_hash != target.content_hash
        || request.successor.target != *target
        || request.successor.runtime_generation != hydrated.snapshot.runtime_generation
    {
        return Err(RuntimeExactPanelReconciliationErrorV2::HydratedIdentityMismatch);
    }
    Ok(())
}

fn execution_guard_v2(execution: &RuntimeExecutionReceiptV1) -> RuntimeExecutionGuardV1 {
    RuntimeExecutionGuardV1 {
        scope: RuntimeDeploymentScopeV1::from_identity(&execution.snapshot.identity),
        expected_revision: execution.snapshot.revision,
        controller_id: execution.controller_id.clone(),
        fencing_token: execution.fencing_token,
        runtime_generation: execution.snapshot.runtime_generation,
        convergence_attempt: execution.convergence_attempt,
    }
}

fn runtime_deadline_v2(
    deadline: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<TokioInstant, RuntimeExactPanelReconciliationErrorV2> {
    let remaining = deadline
        .signed_duration_since(now)
        .to_std()
        .map_err(|_| RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)?;
    TokioInstant::now()
        .checked_add(remaining)
        .ok_or(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_runtime_convergence::{
        ControllerId, ControllerLeaseV1, FencingToken, ProcessInstanceId, RuntimeDeployment,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
        RuntimeProcessIdentityV1,
    };
    use serde_json::json;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn exact_request() -> RuntimeExactPanelReconciliationRequestV2 {
        let identity: RuntimeDeploymentIdentityV1 = serde_json::from_value(json!({
            "deployment_id": "deployment:1",
            "tenant_id": "tenant:1",
            "installation_id": "installation:1",
            "promotion_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "activation_request_id": "activation:1"
        }))
        .unwrap();
        let target: RuntimeDeploymentTargetV1 = serde_json::from_value(json!({
            "guild_id": "7",
            "ruleset_key": "studyroom",
            "version": 1,
            "content_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "binding_revision": 1,
            "binding_fingerprint":
                "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        }))
        .unwrap();
        let mut snapshot = RuntimeDeployment::request(
            identity,
            target.clone(),
            RuntimeGeneration::FIRST,
            None,
            at(80),
        )
        .unwrap()
        .snapshot();
        let controller_id = ControllerId::parse("controller:1").unwrap();
        snapshot.phase = RuntimeDeploymentPhaseV1::ReconcilingPanels;
        snapshot.controller_lease = Some(ControllerLeaseV1 {
            controller_id: controller_id.clone(),
            fencing_token: FencingToken::FIRST,
            acquired_at: at(90),
            expires_at: at(180),
        });
        let execution = RuntimeExecutionReceiptV1 {
            snapshot,
            controller_id,
            fencing_token: FencingToken::FIRST,
            convergence_attempt: NonZeroU32::MIN,
            acquired_at: at(90),
            expires_at: at(180),
        };
        let successor = RuntimeProcessIdentityV1 {
            target,
            runtime_generation: RuntimeGeneration::FIRST,
            process_instance_id: ProcessInstanceId::parse("runtime:1").unwrap(),
        };
        RuntimeExactPanelReconciliationRequestV2::new(execution, successor, at(160))
    }

    #[test]
    fn operation_window_reserves_exact_side_effect_headroom() {
        assert!(validate_operation_window_v2(at(100), at(160), at(180)).is_ok());
        assert!(matches!(
            validate_operation_window_v2(at(100), at(161), at(180)),
            Err(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)
        ));
    }

    #[test]
    fn operation_window_rejects_elapsed_or_non_positive_deadline() {
        for deadline in [at(99), at(100)] {
            assert!(matches!(
                validate_operation_window_v2(at(100), deadline, at(180)),
                Err(RuntimeExactPanelReconciliationErrorV2::InvalidDeadline)
            ));
        }
    }

    #[test]
    fn exact_request_binds_successor_execution_lease_and_phase() {
        let request = exact_request();
        assert!(validate_request_v2(&request, at(100)).is_ok());
        let guard = execution_guard_v2(&request.execution);
        assert!(guard.scope.matches(&request.execution.snapshot.identity));
        assert_eq!(guard.expected_revision, request.execution.snapshot.revision);
        assert_eq!(guard.controller_id, request.execution.controller_id);
        assert_eq!(guard.fencing_token, request.execution.fencing_token);
        assert_eq!(
            guard.runtime_generation,
            request.successor.runtime_generation
        );
    }

    #[test]
    fn exact_request_rejects_lease_or_successor_drift() {
        let mut lease_drift = exact_request();
        lease_drift.execution.expires_at = at(179);
        assert!(matches!(
            validate_request_v2(&lease_drift, at(100)),
            Err(RuntimeExactPanelReconciliationErrorV2::RequestIdentityMismatch)
        ));
        let mut successor_drift = exact_request();
        successor_drift.successor.runtime_generation =
            successor_drift.successor.runtime_generation.next().unwrap();
        assert!(matches!(
            validate_request_v2(&successor_drift, at(100)),
            Err(RuntimeExactPanelReconciliationErrorV2::RequestIdentityMismatch)
        ));
    }

    #[test]
    fn reconciliation_errors_expose_stable_redacted_classification() {
        let error = RuntimeExactPanelReconciliationErrorV2::Claim(
            RuntimePanelPersistenceErrorV1::Indeterminate,
        );
        assert_eq!(error.code(), "runtime_panel_indeterminate");
        assert_eq!(
            error.disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
        );
        assert!(!format!("{error:?}").contains("discord"));
        assert_eq!(
            RuntimeExactPanelReconciliationErrorV2::DeadlineElapsed.disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::ObservationRequired
        );
        assert_eq!(
            RuntimeExactPanelReconciliationErrorV2::Claim(
                RuntimePanelPersistenceErrorV1::AuthorityChanged,
            )
            .disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::AuthorityDrift
        );
        assert_eq!(
            RuntimeExactPanelReconciliationErrorV2::InvalidDeadline.disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::ProcessInvariant
        );
        assert_eq!(
            RuntimeExactPanelReconciliationErrorV2::Certificate(
                CertifiedPanelEvidenceErrorV1::ZeroGuildId,
            )
            .disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
        );
    }

    #[test]
    fn ineligible_reports_preserve_retry_observation_and_blocking_reasons() {
        let mut transient = StrictPanelReportV1 {
            declared_count: 1,
            skipped_transient_count: 1,
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            RuntimePanelIneligibilityV2::from_report(&transient),
            RuntimePanelIneligibilityV2::Transient
        );
        transient.ambiguous_outcome_count = 1;
        assert_eq!(
            RuntimePanelIneligibilityV2::from_report(&transient),
            RuntimePanelIneligibilityV2::Ambiguous
        );
        let cleanup = StrictPanelReportV1 {
            declared_count: 1,
            orphan_message_cleanup_pending_count: 1,
            ..StrictPanelReportV1::default()
        };
        assert_eq!(
            RuntimePanelIneligibilityV2::from_report(&cleanup),
            RuntimePanelIneligibilityV2::CleanupPending
        );
        let unresolved = RuntimeExactPanelReconciliationErrorV2::Ineligible {
            reason: RuntimePanelIneligibilityV2::UnresolvedChannel,
        };
        assert_eq!(unresolved.code(), "runtime_panel_channel_unresolved");
        assert_eq!(
            unresolved.disposition_v2(),
            RuntimeExactPanelReconciliationDispositionV2::DeploymentBlocked
        );
    }
}
