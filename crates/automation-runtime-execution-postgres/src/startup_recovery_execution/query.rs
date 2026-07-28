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

pub(crate) const SELECT_PENDING_DRAIN_STARTUP_RECOVERY_QUERY: &str =
    "SELECT selection_outcome_name, observed_database_now, observed_owner_expires_at, \
            selected_drain_intent_id, selected_source_intent_revision, \
            selected_source_state_digest, selected_slot_guild_id, \
            selected_slot_ruleset_key, selected_target_version, \
            selected_target_content_hash, selected_target_binding_revision, \
            selected_target_binding_fingerprint \
     FROM public.starring_runtime_startup_recovery_select_pending_drain_v2(\
        $1,$2,$3,$4,$5,$6,$7\
     )";

pub(crate) const SELECT_PENDING_DRAIN_STARTUP_RECOVERY_V3_QUERY: &str =
    "SELECT selection_outcome_name, observed_database_now, observed_owner_expires_at, \
            selected_drain_intent_id, selected_source_intent_revision, \
            selected_source_state_digest, selected_source_state_bytes, \
            selected_product_operation_id, selected_product_mutation_digest, \
            selected_tenant_id, selected_installation_id, selected_deployment_id, \
            selected_expected_revision, selected_product_mutation_request_bytes, \
            selected_drain_intent_request_bytes, selected_drain_intent_digest, \
            selected_slot_guild_id, selected_slot_ruleset_key, selected_target_version, \
            selected_target_content_hash, selected_target_binding_revision, \
            selected_target_binding_fingerprint, predecessor_claim_terminal_digest, \
            predecessor_gateway_shard_id, predecessor_process_instance_id, \
            predecessor_lease_epoch, predecessor_runtime_build_revision, \
            predecessor_owner_revision, predecessor_controller_id, \
            predecessor_controller_fencing_token, predecessor_claim_epoch, \
            predecessor_claim_revision, predecessor_claim_expires_at, \
            predecessor_seal_process_instance_id, predecessor_seal_generation, \
            predecessor_seal_observation_sequence \
     FROM public.starring_runtime_startup_recovery_select_pending_drain_v3(\
        $1,$2,$3,$4,$5,$6,$7\
     )";

pub(crate) const RECORD_PENDING_DRAIN_NO_CANDIDATE_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_record_pending_drain_none_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
        $13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24\
     )";

pub(crate) const EXECUTE_PENDING_DRAIN_STARTUP_RECOVERY_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_execute_pending_drain_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
        $13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,\
        $25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,\
        $37,$38,$39,$40,$41,$42,$43,$44,$45,$46,$47,$48\
     )";

pub(crate) const EXECUTE_PENDING_DRAIN_SUCCESSION_STARTUP_RECOVERY_V3_QUERY: &str =
    "SELECT journal_outcome_name, terminal_outcome_name, recovery_id, \
            originating_emergency_generation, coordinator_generation, \
            action_authority_revision, selection_authority_revision, recovery_class, \
            observed_gateway_shard_id, observed_process_instance_id, observed_lease_epoch, \
            observed_runtime_build_revision, observed_owner_revision, database_now, \
            observed_owner_expires_at, minimum_database_now, recorded_at, \
            terminal_projection_bytes, terminal_digest \
     FROM public.starring_runtime_startup_recovery_pending_drain_succession_v3(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,\
        $13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,\
        $25,$26,$27,$28,$29,$30,$31,$32,$33,$34,$35,$36,\
        $37,$38,$39,$40,$41,$42,$43,$44,$45\
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

    #[test]
    fn pending_drain_queries_are_function_only_and_positionally_exact() {
        for (query, function, bind_count) in [
            (
                SELECT_PENDING_DRAIN_STARTUP_RECOVERY_QUERY,
                "public.starring_runtime_startup_recovery_select_pending_drain_v2",
                7,
            ),
            (
                RECORD_PENDING_DRAIN_NO_CANDIDATE_QUERY,
                "public.starring_runtime_startup_recovery_record_pending_drain_none_v2",
                24,
            ),
            (
                EXECUTE_PENDING_DRAIN_STARTUP_RECOVERY_QUERY,
                "public.starring_runtime_startup_recovery_execute_pending_drain_v2",
                48,
            ),
        ] {
            assert_eq!(query.matches('$').count(), bind_count);
            assert!(query.contains(function));
            for forbidden in [
                "runtime_gateway_owners",
                "runtime_drain_intents_v2",
                "runtime_deployments",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "TRUNCATE ",
            ] {
                assert!(!query.contains(forbidden));
            }
        }
        for column in [
            "selected_drain_intent_id",
            "selected_source_intent_revision",
            "selected_source_state_digest",
            "selected_slot_guild_id",
            "selected_slot_ruleset_key",
            "selected_target_version",
            "selected_target_content_hash",
            "selected_target_binding_revision",
            "selected_target_binding_fingerprint",
        ] {
            assert!(
                SELECT_PENDING_DRAIN_STARTUP_RECOVERY_QUERY.contains(column),
                "{column}"
            );
        }
    }

    #[test]
    fn pending_drain_v3_queries_are_function_only_and_positionally_exact() {
        let selector_columns = [
            "selection_outcome_name",
            "observed_database_now",
            "observed_owner_expires_at",
            "selected_drain_intent_id",
            "selected_source_intent_revision",
            "selected_source_state_digest",
            "selected_source_state_bytes",
            "selected_product_operation_id",
            "selected_product_mutation_digest",
            "selected_tenant_id",
            "selected_installation_id",
            "selected_deployment_id",
            "selected_expected_revision",
            "selected_product_mutation_request_bytes",
            "selected_drain_intent_request_bytes",
            "selected_drain_intent_digest",
            "selected_slot_guild_id",
            "selected_slot_ruleset_key",
            "selected_target_version",
            "selected_target_content_hash",
            "selected_target_binding_revision",
            "selected_target_binding_fingerprint",
            "predecessor_claim_terminal_digest",
            "predecessor_gateway_shard_id",
            "predecessor_process_instance_id",
            "predecessor_lease_epoch",
            "predecessor_runtime_build_revision",
            "predecessor_owner_revision",
            "predecessor_controller_id",
            "predecessor_controller_fencing_token",
            "predecessor_claim_epoch",
            "predecessor_claim_revision",
            "predecessor_claim_expires_at",
            "predecessor_seal_process_instance_id",
            "predecessor_seal_generation",
            "predecessor_seal_observation_sequence",
        ];
        let execution_columns = [
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
        ];
        for (query, function, bind_count, columns) in [
            (
                SELECT_PENDING_DRAIN_STARTUP_RECOVERY_V3_QUERY,
                "public.starring_runtime_startup_recovery_select_pending_drain_v3",
                7,
                selector_columns.as_slice(),
            ),
            (
                EXECUTE_PENDING_DRAIN_SUCCESSION_STARTUP_RECOVERY_V3_QUERY,
                "public.starring_runtime_startup_recovery_pending_drain_succession_v3",
                45,
                execution_columns.as_slice(),
            ),
        ] {
            assert_bind_sequence(query, bind_count);
            assert_eq!(selected_columns(query), columns);
            assert_eq!(query.matches(" FROM ").count(), 1);
            assert!(query.contains(&format!(" FROM {function}(")));
            for forbidden in [
                "runtime_gateway_owners",
                "runtime_drain_intents_v2",
                "runtime_product_operations_v2",
                "runtime_deployments",
                "runtime_serving_leases",
                concat!("runtime_startup_", "recovery_actions_v2"),
                "runtime_slot_writer_fences_v2",
                "runtime_writer_fence ",
                "runtime_certification_operations_v2",
                "runtime_certification_operation_terminals_v2",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "TRUNCATE ",
            ] {
                assert!(!query.contains(forbidden), "{forbidden}");
            }
        }
    }

    fn assert_bind_sequence(query: &str, bind_count: usize) {
        let actual = query
            .match_indices('$')
            .map(|(index, _)| {
                query[index + 1..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse::<usize>()
                    .expect("query bind is numeric")
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, (1..=bind_count).collect::<Vec<_>>());
    }

    fn selected_columns(query: &str) -> Vec<&str> {
        query
            .strip_prefix("SELECT ")
            .and_then(|query| query.split_once(" FROM "))
            .map(|(columns, _)| columns.split(',').map(str::trim).collect())
            .expect("query has a SELECT projection and one function FROM")
    }
}
