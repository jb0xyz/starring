use std::time::{Duration, Instant};

use automation_runtime_controller::{
    RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseObservationV1,
    RuntimeGatewayOwnerLeaseReceiptV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
    RuntimeRenewGatewayOwnerLeaseV1,
};

use crate::{
    accept_gateway_owner_renew_v1, RuntimeAcceptedGatewayOwnerReceiptV1,
    RuntimeAcceptedGatewayOwnerRenewV1, RuntimeGatewayOwnerProtocolViolationV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerRenewalPolicyV1 {
    renew_before: Duration,
    safety_margin: Duration,
}

impl RuntimeGatewayOwnerRenewalPolicyV1 {
    pub fn new(
        renew_before: Duration,
        safety_margin: Duration,
    ) -> Result<Self, RuntimeGatewayOwnerRenewalPolicyErrorV1> {
        if renew_before.is_zero() || safety_margin.is_zero() {
            return Err(RuntimeGatewayOwnerRenewalPolicyErrorV1::ZeroDuration);
        }
        if safety_margin >= renew_before {
            return Err(RuntimeGatewayOwnerRenewalPolicyErrorV1::InvalidOrder);
        }
        Ok(Self {
            renew_before,
            safety_margin,
        })
    }

    pub fn renew_before(self) -> Duration {
        self.renew_before
    }

    pub fn safety_margin(self) -> Duration {
        self.safety_margin
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerRenewalPolicyErrorV1 {
    #[error("runtime gateway owner renewal policy contains a zero duration")]
    ZeroDuration,
    #[error("runtime gateway owner renewal policy order is invalid")]
    InvalidOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerWatchdogActionV1 {
    WaitUntil(Instant),
    RenewNow,
    InvalidateNow,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerRenewalScheduleV1 {
    receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    policy: RuntimeGatewayOwnerRenewalPolicyV1,
    request_started_at: Instant,
    response_observed_at: Instant,
    renew_at: Instant,
    safety_deadline: Instant,
    conservative_expiry: Instant,
}

impl RuntimeGatewayOwnerRenewalScheduleV1 {
    fn from_receipt(
        receipt: RuntimeGatewayOwnerLeaseReceiptV1,
        policy: RuntimeGatewayOwnerRenewalPolicyV1,
        request_started_at: Instant,
        response_observed_at: Instant,
    ) -> Result<Self, RuntimeGatewayOwnerRenewalScheduleErrorV1> {
        if response_observed_at < request_started_at {
            return Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::ClockReversed);
        }
        let lease_duration = receipt
            .database_lease_duration()
            .ok_or(RuntimeGatewayOwnerRenewalScheduleErrorV1::NonFreshReceipt)?;
        if policy.renew_before >= lease_duration {
            return Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::LeaseTooShort);
        }
        let conservative_expiry = request_started_at
            .checked_add(lease_duration)
            .ok_or(RuntimeGatewayOwnerRenewalScheduleErrorV1::InstantOverflow)?;
        let renew_at = conservative_expiry
            .checked_sub(policy.renew_before)
            .ok_or(RuntimeGatewayOwnerRenewalScheduleErrorV1::InstantOverflow)?;
        let safety_deadline = conservative_expiry
            .checked_sub(policy.safety_margin)
            .ok_or(RuntimeGatewayOwnerRenewalScheduleErrorV1::InstantOverflow)?;
        if response_observed_at >= safety_deadline {
            return Err(RuntimeGatewayOwnerRenewalScheduleErrorV1::SafetyElapsed);
        }
        Ok(Self {
            receipt,
            policy,
            request_started_at,
            response_observed_at,
            renew_at,
            safety_deadline,
            conservative_expiry,
        })
    }

    pub fn receipt(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.receipt
    }

    pub fn policy(&self) -> RuntimeGatewayOwnerRenewalPolicyV1 {
        self.policy
    }

    pub fn request_started_at(&self) -> Instant {
        self.request_started_at
    }

    pub fn response_observed_at(&self) -> Instant {
        self.response_observed_at
    }

    pub fn renew_at(&self) -> Instant {
        self.renew_at
    }

    pub fn safety_deadline(&self) -> Instant {
        self.safety_deadline
    }

    pub fn conservative_expiry(&self) -> Instant {
        self.conservative_expiry
    }

    pub fn action_at(&self, now: Instant) -> RuntimeGatewayOwnerWatchdogActionV1 {
        if now >= self.safety_deadline {
            RuntimeGatewayOwnerWatchdogActionV1::InvalidateNow
        } else if now >= self.renew_at {
            RuntimeGatewayOwnerWatchdogActionV1::RenewNow
        } else {
            RuntimeGatewayOwnerWatchdogActionV1::WaitUntil(self.renew_at)
        }
    }

    pub fn safe_remaining_at(&self, now: Instant) -> Option<Duration> {
        self.safety_deadline
            .checked_duration_since(now)
            .filter(|duration| !duration.is_zero())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerRenewalScheduleErrorV1 {
    #[error("runtime gateway owner receipt is not fresh")]
    NonFreshReceipt,
    #[error("runtime gateway owner receipt is shorter than the renewal window")]
    LeaseTooShort,
    #[error("runtime gateway owner monotonic clock order is invalid")]
    ClockReversed,
    #[error("runtime gateway owner monotonic deadline overflowed")]
    InstantOverflow,
    #[error("runtime gateway owner safety deadline elapsed")]
    SafetyElapsed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerWatchdogV1 {
    schedule: RuntimeGatewayOwnerRenewalScheduleV1,
}

impl RuntimeGatewayOwnerWatchdogV1 {
    pub fn from_accepted_receipt(
        accepted_receipt: RuntimeAcceptedGatewayOwnerReceiptV1,
        policy: RuntimeGatewayOwnerRenewalPolicyV1,
        request_started_at: Instant,
        response_observed_at: Instant,
    ) -> Result<Self, RuntimeGatewayOwnerRenewalScheduleErrorV1> {
        Ok(Self {
            schedule: RuntimeGatewayOwnerRenewalScheduleV1::from_receipt(
                accepted_receipt.into_receipt(),
                policy,
                request_started_at,
                response_observed_at,
            )?,
        })
    }

    pub fn schedule(&self) -> &RuntimeGatewayOwnerRenewalScheduleV1 {
        &self.schedule
    }

    pub fn action_at(&self, now: Instant) -> RuntimeGatewayOwnerWatchdogActionV1 {
        self.schedule.action_at(now)
    }

    pub fn begin_renewal(
        self,
        lease_for: RuntimeGatewayOwnerLeaseDurationV1,
        request_started_at: Instant,
    ) -> Result<RuntimeGatewayOwnerRenewalInFlightV1, RuntimeGatewayOwnerWatchdogErrorV1> {
        if request_started_at < self.schedule.response_observed_at {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed);
        }
        if request_started_at >= self.schedule.safety_deadline {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed);
        }
        if lease_for.get() <= self.schedule.policy.renew_before {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::RequestedLeaseTooShort);
        }
        if self.schedule.receipt.owner_revision == std::num::NonZeroU64::MAX {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::RevisionExhausted);
        }
        let request = RuntimeRenewGatewayOwnerLeaseV1 {
            lease_id: self.schedule.receipt.lease_id.clone(),
            expected_owner_revision: self.schedule.receipt.owner_revision,
            lease_for,
        };
        Ok(RuntimeGatewayOwnerRenewalInFlightV1 {
            previous_schedule: self.schedule,
            request,
            request_started_at,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerRenewalInFlightV1 {
    previous_schedule: RuntimeGatewayOwnerRenewalScheduleV1,
    request: RuntimeRenewGatewayOwnerLeaseV1,
    request_started_at: Instant,
}

impl RuntimeGatewayOwnerRenewalInFlightV1 {
    pub fn request(&self) -> &RuntimeRenewGatewayOwnerLeaseV1 {
        &self.request
    }

    pub fn previous_schedule(&self) -> &RuntimeGatewayOwnerRenewalScheduleV1 {
        &self.previous_schedule
    }

    pub fn complete(
        self,
        outcome: RuntimeRenewGatewayOwnerLeaseOutcomeV1,
        response_observed_at: Instant,
    ) -> Result<RuntimeGatewayOwnerRenewalCompletionV1, RuntimeGatewayOwnerWatchdogErrorV1> {
        if response_observed_at < self.request_started_at {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed);
        }
        if response_observed_at >= self.previous_schedule.safety_deadline {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed);
        }
        let accepted =
            accept_gateway_owner_renew_v1(&self.request, outcome).map_err(|violation| {
                RuntimeGatewayOwnerWatchdogErrorV1::ProtocolViolation { violation }
            })?;
        match accepted {
            RuntimeAcceptedGatewayOwnerRenewV1::Renewed(receipt) => {
                let schedule = RuntimeGatewayOwnerRenewalScheduleV1::from_receipt(
                    receipt,
                    self.previous_schedule.policy,
                    self.request_started_at,
                    response_observed_at,
                )
                .map_err(RuntimeGatewayOwnerWatchdogErrorV1::Schedule)?;
                let watchdog = RuntimeGatewayOwnerWatchdogV1 { schedule };
                Ok(RuntimeGatewayOwnerRenewalCompletionV1::Renewed(watchdog))
            }
            RuntimeAcceptedGatewayOwnerRenewV1::OwnershipLost(observation) => Ok(
                RuntimeGatewayOwnerRenewalCompletionV1::OwnershipLost(observation),
            ),
        }
    }

    pub fn definitely_not_applied(
        self,
        response_observed_at: Instant,
    ) -> Result<RuntimeGatewayOwnerWatchdogV1, RuntimeGatewayOwnerWatchdogErrorV1> {
        if response_observed_at < self.request_started_at {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::ClockReversed);
        }
        if response_observed_at >= self.previous_schedule.safety_deadline {
            return Err(RuntimeGatewayOwnerWatchdogErrorV1::SafetyElapsed);
        }
        Ok(RuntimeGatewayOwnerWatchdogV1 {
            schedule: self.previous_schedule,
        })
    }

    pub fn into_unknown(self) -> RuntimeGatewayOwnerUnknownRenewalV1 {
        RuntimeGatewayOwnerUnknownRenewalV1 {
            previous_schedule: self.previous_schedule,
            request: self.request,
            request_started_at: self.request_started_at,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeGatewayOwnerRenewalCompletionV1 {
    Renewed(RuntimeGatewayOwnerWatchdogV1),
    OwnershipLost(RuntimeGatewayOwnerLeaseObservationV1),
}

#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeGatewayOwnerUnknownRenewalV1 {
    previous_schedule: RuntimeGatewayOwnerRenewalScheduleV1,
    request: RuntimeRenewGatewayOwnerLeaseV1,
    request_started_at: Instant,
}

impl RuntimeGatewayOwnerUnknownRenewalV1 {
    pub fn request(&self) -> &RuntimeRenewGatewayOwnerLeaseV1 {
        &self.request
    }

    pub fn previous_schedule(&self) -> &RuntimeGatewayOwnerRenewalScheduleV1 {
        &self.previous_schedule
    }

    pub fn request_started_at(&self) -> Instant {
        self.request_started_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeGatewayOwnerWatchdogErrorV1 {
    #[error("runtime gateway owner watchdog monotonic clock order is invalid")]
    ClockReversed,
    #[error("runtime gateway owner watchdog safety deadline elapsed")]
    SafetyElapsed,
    #[error("runtime gateway owner requested lease is shorter than its renewal window")]
    RequestedLeaseTooShort,
    #[error("runtime gateway owner revision exhausted")]
    RevisionExhausted,
    #[error("runtime gateway owner renewal response violated its protocol")]
    ProtocolViolation {
        violation: RuntimeGatewayOwnerProtocolViolationV1,
    },
    #[error("runtime gateway owner renewal schedule is invalid")]
    Schedule(RuntimeGatewayOwnerRenewalScheduleErrorV1),
}
