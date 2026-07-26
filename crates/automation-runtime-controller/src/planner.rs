use std::time::Duration;

use automation_runtime_convergence::{
    ControllerId, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1, RuntimePendingConditionV1,
};
use chrono::{DateTime, Utc};

use crate::RuntimeControllerConfigV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeControllerStopReasonV1 {
    Blocked,
    Superseded,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeControllerActionV1 {
    RenewControllerLease {
        lease_for: Duration,
    },
    VerifyPreflight {
        timeout: Duration,
    },
    RequestDrain,
    DrainPreviousRuntime {
        timeout: Duration,
    },
    BeginActivation,
    VerifyActiveTarget {
        timeout: Duration,
    },
    WaitForRetry {
        not_before: DateTime<Utc>,
    },
    ResumeRuntimePending,
    BeginPanelReconciliation,
    ReconcilePanels {
        timeout: Duration,
    },
    StartGatewayAndCertifyLive {
        timeout: Duration,
    },
    MonitorServing {
        heartbeat_every: Duration,
        lease_for: Duration,
    },
    Stop {
        reason: RuntimeControllerStopReasonV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeControllerPlanError {
    #[error("runtime controller configuration is invalid")]
    InvalidConfiguration,
    #[error("runtime deployment requires a controller lease")]
    LeaseMissing,
    #[error("runtime deployment is leased by another controller")]
    ControllerMismatch,
    #[error("runtime controller lease has expired")]
    LeaseExpired,
}

pub fn plan_runtime_action_v1(
    snapshot: &RuntimeDeploymentSnapshotV1,
    controller_id: &ControllerId,
    now: DateTime<Utc>,
    config: &RuntimeControllerConfigV1,
) -> Result<RuntimeControllerActionV1, RuntimeControllerPlanError> {
    config
        .validate()
        .map_err(|_| RuntimeControllerPlanError::InvalidConfiguration)?;
    match &snapshot.phase {
        RuntimeDeploymentPhaseV1::Live => {
            return Ok(RuntimeControllerActionV1::MonitorServing {
                heartbeat_every: config.serving_heartbeat_every,
                lease_for: config.serving_lease_for,
            });
        }
        RuntimeDeploymentPhaseV1::Superseded { .. } => {
            return Ok(RuntimeControllerActionV1::Stop {
                reason: RuntimeControllerStopReasonV1::Superseded,
            });
        }
        RuntimeDeploymentPhaseV1::Cancelled { .. } => {
            return Ok(RuntimeControllerActionV1::Stop {
                reason: RuntimeControllerStopReasonV1::Cancelled,
            });
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        } => {
            return Ok(RuntimeControllerActionV1::Stop {
                reason: RuntimeControllerStopReasonV1::Blocked,
            });
        }
        _ => {}
    }
    let lease = snapshot
        .controller_lease
        .as_ref()
        .ok_or(RuntimeControllerPlanError::LeaseMissing)?;
    if lease.controller_id != *controller_id {
        return Err(RuntimeControllerPlanError::ControllerMismatch);
    }
    if lease.expires_at <= now {
        return Err(RuntimeControllerPlanError::LeaseExpired);
    }
    let renew_at = lease.expires_at
        - chrono::TimeDelta::from_std(config.controller_renew_before)
            .map_err(|_| RuntimeControllerPlanError::InvalidConfiguration)?;
    if now >= renew_at {
        return Ok(RuntimeControllerActionV1::RenewControllerLease {
            lease_for: config.controller_lease_for,
        });
    }
    Ok(match &snapshot.phase {
        RuntimeDeploymentPhaseV1::Requested => RuntimeControllerActionV1::VerifyPreflight {
            timeout: config.preflight_timeout,
        },
        RuntimeDeploymentPhaseV1::PreflightReady => RuntimeControllerActionV1::RequestDrain,
        RuntimeDeploymentPhaseV1::DrainRequested => {
            RuntimeControllerActionV1::DrainPreviousRuntime {
                timeout: config.drain_timeout,
            }
        }
        RuntimeDeploymentPhaseV1::Drained => RuntimeControllerActionV1::BeginActivation,
        RuntimeDeploymentPhaseV1::ActivationApplying => {
            RuntimeControllerActionV1::VerifyActiveTarget {
                timeout: config.activation_timeout,
            }
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Ready,
        } => RuntimeControllerActionV1::BeginPanelReconciliation,
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition:
                RuntimePendingConditionV1::Retryable {
                    retry_not_before, ..
                },
        } if *retry_not_before > now => RuntimeControllerActionV1::WaitForRetry {
            not_before: *retry_not_before,
        },
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Retryable { .. },
        } => RuntimeControllerActionV1::ResumeRuntimePending,
        RuntimeDeploymentPhaseV1::ReconcilingPanels => RuntimeControllerActionV1::ReconcilePanels {
            timeout: config.panel_reconciliation_timeout,
        },
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady => {
            RuntimeControllerActionV1::StartGatewayAndCertifyLive {
                timeout: config.gateway_ready_timeout,
            }
        }
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked { .. },
        }
        | RuntimeDeploymentPhaseV1::Live
        | RuntimeDeploymentPhaseV1::Superseded { .. }
        | RuntimeDeploymentPhaseV1::Cancelled { .. } => unreachable!(),
    })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationRequestId, BindingRevision, ControllerLeaseV1, DeploymentId, DeploymentRevision,
        FencingToken, InstallationId, PromotionId, RuntimeDeploymentIdentityV1,
        RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1,
        RuntimeGeneration, TenantId,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;

    fn at(second: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(second, 0).unwrap()
    }

    fn snapshot(phase: RuntimeDeploymentPhaseV1) -> RuntimeDeploymentSnapshotV1 {
        RuntimeDeploymentSnapshotV1 {
            identity: RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("deployment").unwrap(),
                tenant_id: TenantId::parse("tenant").unwrap(),
                installation_id: InstallationId::parse("installation").unwrap(),
                promotion_id: PromotionId::parse("1".repeat(64)).unwrap(),
                activation_request_id: ActivationRequestId::parse("activation").unwrap(),
            },
            target: RuntimeDeploymentTargetV1 {
                guild_id: GuildId(1),
                ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
                version: RuleSetVersionId::new(1).unwrap(),
                content_hash: RuleSetContentHash::parse_hex(&"2".repeat(64)).unwrap(),
                binding_revision: BindingRevision::new(1).unwrap(),
                binding_fingerprint: ResourceBindingFingerprint::parse(&"3".repeat(64)).unwrap(),
            },
            runtime_generation: RuntimeGeneration::new(1).unwrap(),
            previous_runtime: None,
            requested_at: at(1),
            revision: DeploymentRevision::FIRST,
            phase,
            controller_lease: Some(ControllerLeaseV1 {
                controller_id: ControllerId::parse("controller").unwrap(),
                fencing_token: FencingToken::new(1).unwrap(),
                acquired_at: at(10),
                expires_at: at(100),
            }),
            last_fencing_token: Some(FencingToken::new(1).unwrap()),
            preflight: None,
            drain: None,
            activation: None,
            panel_certificate: None,
            gateway_ready: None,
            live: None,
            last_live_recovery: None,
            last_runtime_failure: None,
        }
    }

    fn plan(
        phase: RuntimeDeploymentPhaseV1,
        now: DateTime<Utc>,
    ) -> Result<RuntimeControllerActionV1, RuntimeControllerPlanError> {
        plan_runtime_action_v1(
            &snapshot(phase),
            &ControllerId::parse("controller").unwrap(),
            now,
            &RuntimeControllerConfigV1::default(),
        )
    }

    #[test]
    fn durable_phases_map_to_one_allowed_action() {
        let cases = [
            (
                RuntimeDeploymentPhaseV1::Requested,
                RuntimeControllerActionV1::VerifyPreflight {
                    timeout: Duration::from_secs(20),
                },
            ),
            (
                RuntimeDeploymentPhaseV1::PreflightReady,
                RuntimeControllerActionV1::RequestDrain,
            ),
            (
                RuntimeDeploymentPhaseV1::DrainRequested,
                RuntimeControllerActionV1::DrainPreviousRuntime {
                    timeout: Duration::from_secs(15),
                },
            ),
            (
                RuntimeDeploymentPhaseV1::Drained,
                RuntimeControllerActionV1::BeginActivation,
            ),
            (
                RuntimeDeploymentPhaseV1::ActivationApplying,
                RuntimeControllerActionV1::VerifyActiveTarget {
                    timeout: Duration::from_secs(10),
                },
            ),
            (
                RuntimeDeploymentPhaseV1::RuntimePending {
                    condition: RuntimePendingConditionV1::Ready,
                },
                RuntimeControllerActionV1::BeginPanelReconciliation,
            ),
            (
                RuntimeDeploymentPhaseV1::ReconcilingPanels,
                RuntimeControllerActionV1::ReconcilePanels {
                    timeout: Duration::from_secs(30),
                },
            ),
            (
                RuntimeDeploymentPhaseV1::AwaitingGatewayReady,
                RuntimeControllerActionV1::StartGatewayAndCertifyLive {
                    timeout: Duration::from_secs(30),
                },
            ),
        ];
        for (phase, expected) in cases {
            assert_eq!(plan(phase, at(20)).unwrap(), expected);
        }
    }

    #[test]
    fn renewal_preempts_external_work_near_expiry() {
        assert_eq!(
            plan(RuntimeDeploymentPhaseV1::Requested, at(70)).unwrap(),
            RuntimeControllerActionV1::RenewControllerLease {
                lease_for: Duration::from_secs(90)
            }
        );
    }

    #[test]
    fn retry_wait_does_not_resume_early() {
        let failure = RuntimeFailureV1 {
            failure_id: RuntimeFailureId::parse("failure").unwrap(),
            kind: RuntimeFailureKindV1::GatewayStart,
            code: "gateway_start".to_string(),
            message: "gateway start failed".to_string(),
            recorded_at: at(15),
        };
        let phase = RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Retryable {
                failure,
                attempt: NonZeroU32::new(1).unwrap(),
                retry_not_before: at(40),
            },
        };
        assert_eq!(
            plan(phase.clone(), at(20)).unwrap(),
            RuntimeControllerActionV1::WaitForRetry { not_before: at(40) }
        );
        assert_eq!(
            plan(phase, at(40)).unwrap(),
            RuntimeControllerActionV1::ResumeRuntimePending
        );
    }

    #[test]
    fn live_is_monitored_without_a_controller_lease() {
        let mut live = snapshot(RuntimeDeploymentPhaseV1::Live);
        live.controller_lease = None;
        assert_eq!(
            plan_runtime_action_v1(
                &live,
                &ControllerId::parse("controller").unwrap(),
                at(200),
                &RuntimeControllerConfigV1::default()
            )
            .unwrap(),
            RuntimeControllerActionV1::MonitorServing {
                heartbeat_every: Duration::from_secs(15),
                lease_for: Duration::from_secs(45)
            }
        );
    }

    #[test]
    fn wrong_or_expired_lease_fails_closed() {
        assert_eq!(
            plan_runtime_action_v1(
                &snapshot(RuntimeDeploymentPhaseV1::Requested),
                &ControllerId::parse("other").unwrap(),
                at(20),
                &RuntimeControllerConfigV1::default()
            ),
            Err(RuntimeControllerPlanError::ControllerMismatch)
        );
        assert_eq!(
            plan(RuntimeDeploymentPhaseV1::Requested, at(100)),
            Err(RuntimeControllerPlanError::LeaseExpired)
        );
    }
}
