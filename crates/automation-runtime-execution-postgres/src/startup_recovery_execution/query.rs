pub(crate) const EXECUTE_STALE_LIVE_STARTUP_RECOVERY_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_execute_stale_live_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12\
     )";

pub(crate) const EXECUTE_RESERVED_AWAITING_STARTUP_RECOVERY_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12\
     )";

pub(crate) const EXECUTE_SUSPENDED_LOCAL_STARTUP_RECOVERY_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_execute_suspended_local_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
        $13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24\
     )";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_query_is_function_only_and_positionally_exact() {
        for (query, function, bind_count) in [
            (
                EXECUTE_STALE_LIVE_STARTUP_RECOVERY_QUERY,
                "public.starring_runtime_startup_recovery_execute_stale_live_v2",
                12,
            ),
            (
                EXECUTE_RESERVED_AWAITING_STARTUP_RECOVERY_QUERY,
                "public.starring_runtime_startup_recovery_execute_reserved_awaiting_v2",
                12,
            ),
            (
                EXECUTE_SUSPENDED_LOCAL_STARTUP_RECOVERY_QUERY,
                "public.starring_runtime_startup_recovery_execute_suspended_local_v2",
                24,
            ),
        ] {
            assert_eq!(query.matches('$').count(), bind_count);
            assert!(query.contains(function));
            for column in [
                "journal_outcome_name",
                "terminal_outcome_name",
                "recovery_id",
                "originating_emergency_generation",
                "coordinator_generation",
                "action_authority_revision",
                "selection_authority_revision",
                "recovery_class",
                "observed_gateway_shard_id",
                "observed_process_instance_id",
                "observed_lease_epoch",
                "observed_runtime_build_revision",
                "observed_owner_revision",
                "database_now",
                "observed_owner_expires_at",
                "minimum_database_now",
                "recorded_at",
                "terminal_projection_bytes",
                "terminal_digest",
            ] {
                assert!(query.contains(column), "{column}");
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
                assert!(!query.contains(forbidden));
            }
        }
    }
}
