pub(crate) const DATABASE_BINDING_QUERY: &str =
    "SELECT public.starring_runtime_execution_database_identity_v1() \
        AS database_identity, pg_catalog.current_database()::TEXT AS database_name, \
        session_user::TEXT AS executor_role";

pub(crate) const CLAIM_NEXT_QUERY: &str =
    "SELECT * FROM public.starring_runtime_execution_claim_next_v1($1, $2)";

pub(crate) const RENEW_QUERY: &str = "SELECT * FROM public.starring_runtime_execution_renew_v1(\
        $1, $2, $3, $4, $5, $6, $7, $8, $9)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_queries_are_function_only_and_positionally_exact() {
        assert_eq!(CLAIM_NEXT_QUERY.matches('$').count(), 2);
        assert_eq!(RENEW_QUERY.matches('$').count(), 9);
        for query in [DATABASE_BINDING_QUERY, CLAIM_NEXT_QUERY, RENEW_QUERY] {
            for forbidden in [
                "runtime_deployments",
                "runtime_attestations",
                "runtime_serving_leases",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
            ] {
                assert!(!query.contains(forbidden));
            }
        }
    }
}
