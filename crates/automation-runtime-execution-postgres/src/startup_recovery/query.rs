pub(crate) const OBSERVE_STARTUP_RECOVERY_QUERY: &str =
    "SELECT outcome_name, observed_gateway_shard_id, observed_process_instance_id, \
            observed_lease_epoch, observed_runtime_build_revision, observed_owner_revision, \
            database_now, observed_owner_expires_at, serving_state_name, serving_count, \
            serving_earliest_expiry, serving_retry_after_milliseconds, \
            recoverable_awaiting_certification_count, suspended_local_effect_count, \
            pending_runtime_drain_intent_count, acknowledged_product_handoff_count \
     FROM public.starring_runtime_startup_recovery_observe_v2($1,$2,$3,$4,$5,$6)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_recovery_query_is_function_only_and_positionally_exact() {
        assert_eq!(OBSERVE_STARTUP_RECOVERY_QUERY.matches('$').count(), 6);
        assert!(OBSERVE_STARTUP_RECOVERY_QUERY
            .contains("public.starring_runtime_startup_recovery_observe_v2"));
        for column in [
            "outcome_name",
            "observed_gateway_shard_id",
            "observed_process_instance_id",
            "observed_lease_epoch",
            "observed_runtime_build_revision",
            "observed_owner_revision",
            "database_now",
            "observed_owner_expires_at",
            "serving_state_name",
            "serving_count",
            "serving_earliest_expiry",
            "serving_retry_after_milliseconds",
            "recoverable_awaiting_certification_count",
            "suspended_local_effect_count",
            "pending_runtime_drain_intent_count",
            "acknowledged_product_handoff_count",
        ] {
            assert!(OBSERVE_STARTUP_RECOVERY_QUERY.contains(column));
        }
        for forbidden in [
            "runtime_gateway_owners",
            "runtime_deployments",
            "runtime_serving_leases",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
            "TRUNCATE ",
        ] {
            assert!(!OBSERVE_STARTUP_RECOVERY_QUERY.contains(forbidden));
        }
    }
}
