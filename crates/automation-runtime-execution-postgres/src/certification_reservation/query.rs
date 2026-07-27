pub(crate) const RESERVE_CERTIFICATION_INTENT_QUERY: &str =
    "SELECT outcome_name, locked_snapshot, locked_convergence_attempt_no, observed_at, \
            operation_id, tenant_id, installation_id, deployment_id, deployment_revision, \
            convergence_attempt_no, certification_intent_bytes, intent_fingerprint \
     FROM public.starring_runtime_certification_reserve_intent_v2(\
        $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,\
        $23,$24,$25,$26,$27\
     )";

pub(crate) const OBSERVE_CERTIFICATION_RESERVATION_SCOPE_QUERY: &str =
    "SELECT outcome_name, locked_snapshot, locked_convergence_attempt_no, observed_at, \
            operation_id, tenant_id, installation_id, deployment_id, deployment_revision, \
            convergence_attempt_no, certification_intent_bytes, intent_fingerprint \
     FROM public.starring_runtime_certification_reservation_observe_v2($1,$2,$3,$4,$5)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_queries_are_function_only_and_positionally_exact() {
        assert_eq!(RESERVE_CERTIFICATION_INTENT_QUERY.matches('$').count(), 27);
        assert_eq!(
            OBSERVE_CERTIFICATION_RESERVATION_SCOPE_QUERY
                .matches('$')
                .count(),
            5
        );
        for query in [
            RESERVE_CERTIFICATION_INTENT_QUERY,
            OBSERVE_CERTIFICATION_RESERVATION_SCOPE_QUERY,
        ] {
            for forbidden in [
                "runtime_certification_operations_v2",
                "runtime_deployments",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
            ] {
                assert!(!query.contains(forbidden));
            }
        }
    }
}
