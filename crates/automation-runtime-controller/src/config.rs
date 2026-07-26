use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeControllerConfigV1 {
    pub controller_lease_for: Duration,
    pub controller_renew_before: Duration,
    pub serving_lease_for: Duration,
    pub serving_heartbeat_every: Duration,
    pub preflight_timeout: Duration,
    pub drain_timeout: Duration,
    pub activation_timeout: Duration,
    pub panel_reconciliation_timeout: Duration,
    pub gateway_ready_timeout: Duration,
}

impl Default for RuntimeControllerConfigV1 {
    fn default() -> Self {
        Self {
            controller_lease_for: Duration::from_secs(90),
            controller_renew_before: Duration::from_secs(30),
            serving_lease_for: Duration::from_secs(45),
            serving_heartbeat_every: Duration::from_secs(15),
            preflight_timeout: Duration::from_secs(20),
            drain_timeout: Duration::from_secs(15),
            activation_timeout: Duration::from_secs(10),
            panel_reconciliation_timeout: Duration::from_secs(30),
            gateway_ready_timeout: Duration::from_secs(30),
        }
    }
}

impl RuntimeControllerConfigV1 {
    pub fn validate(&self) -> Result<(), RuntimeControllerConfigError> {
        let durations = [
            self.controller_lease_for,
            self.controller_renew_before,
            self.serving_lease_for,
            self.serving_heartbeat_every,
            self.preflight_timeout,
            self.drain_timeout,
            self.activation_timeout,
            self.panel_reconciliation_timeout,
            self.gateway_ready_timeout,
        ];
        if durations.iter().any(Duration::is_zero) {
            return Err(RuntimeControllerConfigError::ZeroDuration);
        }
        if self.controller_lease_for > Duration::from_secs(600)
            || self.serving_lease_for > Duration::from_secs(300)
            || durations[4..]
                .iter()
                .any(|duration| *duration > Duration::from_secs(300))
        {
            return Err(RuntimeControllerConfigError::DurationTooLarge);
        }
        if self.controller_renew_before >= self.controller_lease_for {
            return Err(RuntimeControllerConfigError::InvalidControllerRenewal);
        }
        let operation_budget = self
            .controller_lease_for
            .checked_sub(self.controller_renew_before)
            .ok_or(RuntimeControllerConfigError::InvalidControllerRenewal)?;
        if durations[4..]
            .iter()
            .any(|duration| *duration > operation_budget)
        {
            return Err(RuntimeControllerConfigError::OperationExceedsLeaseBudget);
        }
        let heartbeat_safety = self
            .serving_heartbeat_every
            .checked_mul(2)
            .ok_or(RuntimeControllerConfigError::InvalidServingHeartbeat)?;
        if heartbeat_safety >= self.serving_lease_for {
            return Err(RuntimeControllerConfigError::InvalidServingHeartbeat);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeControllerConfigError {
    #[error("runtime controller durations must be non-zero")]
    ZeroDuration,
    #[error("runtime controller duration exceeds its safety limit")]
    DurationTooLarge,
    #[error("runtime controller renewal window is invalid")]
    InvalidControllerRenewal,
    #[error("runtime controller operation timeout exceeds the leased work budget")]
    OperationExceedsLeaseBudget,
    #[error("runtime serving heartbeat does not leave two full safety intervals")]
    InvalidServingHeartbeat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_lease_and_heartbeat_safety() {
        assert_eq!(RuntimeControllerConfigV1::default().validate(), Ok(()));
    }

    #[test]
    fn operation_must_fit_before_the_renewal_window() {
        let config = RuntimeControllerConfigV1 {
            gateway_ready_timeout: Duration::from_secs(61),
            ..RuntimeControllerConfigV1::default()
        };
        assert_eq!(
            config.validate(),
            Err(RuntimeControllerConfigError::OperationExceedsLeaseBudget)
        );
    }

    #[test]
    fn heartbeat_requires_more_than_two_intervals() {
        let config = RuntimeControllerConfigV1 {
            serving_lease_for: Duration::from_secs(30),
            ..RuntimeControllerConfigV1::default()
        };
        assert_eq!(
            config.validate(),
            Err(RuntimeControllerConfigError::InvalidServingHeartbeat)
        );
    }
}
